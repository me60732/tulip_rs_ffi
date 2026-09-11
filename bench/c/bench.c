// C benchmark harness for the tulip_rs_ffi (hand-rolled extern "C") bindings.
//
// Mirrors the methodology used by tulip_rs_diplomat/bench/c, tulip_rs_python/bench
// and tulip_rs_node/bench:
//   - Same 4 stocks, same 6,705-bar real OHLCV history (fetched live from Postgres via libpq)
//   - Same 4 option sets per indicator
//   - warmup + (repeat independent samples, each averaging `number` back-to-back
//     calls) -> mean/stddev/min/max in nanoseconds
//   - Results logged to the shared `indicator_benchmark` Postgres database under
//     implementation_type = 'tulip_rs_ffi_c', via direct libpq calls.
//
// Each timed `bench_<name>` call performs one full call-and-destroy cycle of
// the wrapper under test: <name>_indicator() (allocation happens inside the
// wrapper) followed by tulip_ffi_result_free() + <name>_state_free(), so the
// numbers include the FFI result packing/teardown cost, not just compute.
//
// Build: see Makefile. Run: see README.md.

#include <ctype.h>
#include <limits.h>
#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include "tulip_rs_ffi.h"

// Reference implementations for comparison -- same two libraries used by the
// core tulip_rs Rust criterion benches (tulip_rs/tulip_test/benches) and by
// the diplomat C bench (tulip_rs_diplomat/bench/c), built from the same git
// submodules (see Makefile): Tulip Indicators' C library
// (implementation_type = "C_tulip") and TA-Lib (implementation_type = "talib").
#include "indicators.h"
#include "ta_libc.h"

// ---------------------------------------------------------------------------
// libpq declarations (hand-declared since no libpq-fe.h header is available)
// ---------------------------------------------------------------------------

typedef struct pg_conn PGconn;
typedef struct pg_result PGresult;

extern PGconn *PQconnectdb(const char *conninfo);
extern int PQstatus(PGconn *conn);
extern char *PQerrorMessage(PGconn *conn);
extern PGresult *PQexec(PGconn *conn, const char *query);
extern int PQresultStatus(PGresult *res);
extern char *PQresultErrorMessage(PGresult *res);
extern int PQntuples(PGresult *res);
extern int PQnfields(PGresult *res);
extern char *PQgetvalue(PGresult *res, int row_num, int field_num);
extern void PQclear(PGresult *res);
extern void PQfinish(PGconn *conn);

// libpq status constants (ABI-level, from PostgreSQL source)
#define CONNECTION_OK 0
#define PGRES_COMMAND_OK 1
#define PGRES_TUPLES_OK 2

// ---------------------------------------------------------------------------
// Config (overridable via environment variables, same names as the Python/Node
// benchmark suites)
// ---------------------------------------------------------------------------

static int env_int(const char *name, int default_value) {
    const char *v = getenv(name);
    return v ? atoi(v) : default_value;
}

static const char *env_str(const char *name, const char *default_value) {
    const char *v = getenv(name);
    return v ? v : default_value;
}

// Loads KEY=VALUE pairs from a .env file without overriding any variable
// already present in the environment (mirrors python-dotenv's default
// behaviour -- real shell exports always win over the .env file).
static void load_dotenv_file(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return;
    char line[1024];
    while (fgets(line, sizeof(line), f)) {
        char *p = line;
        while (isspace((unsigned char) *p)) p++;
        if (*p == '#' || *p == '\0' || *p == '\n') continue;
        char *eq = strchr(p, '=');
        if (!eq) continue;
        *eq = '\0';
        char *key = p;
        char *val = eq + 1;
        char *kend = key + strlen(key);
        while (kend > key && isspace((unsigned char) kend[-1])) *--kend = '\0';
        while (isspace((unsigned char) *val)) val++;
        char *vend = val + strlen(val);
        while (vend > val && (isspace((unsigned char) vend[-1]) || vend[-1] == '\n' || vend[-1] == '\r')) *--vend = '\0';
        if (vend - val >= 2 && ((val[0] == '"' && vend[-1] == '"') || (val[0] == '\'' && vend[-1] == '\''))) {
            val[strlen(val) - 1] = '\0';
            val++;
        }
        setenv(key, val, 0); // 0 = do not overwrite an already-set env var
    }
    fclose(f);
}

// Walks upward from CWD looking for `.env`, mirroring python-dotenv's
// find_dotenv(usecwd=True) behaviour used by every other tulip-rs bench suite.
static void load_dotenv(void) {
    const char *override = getenv("DOTENV_PATH");
    if (override) { load_dotenv_file(override); return; }
    char dir[PATH_MAX];
    if (!getcwd(dir, sizeof(dir))) return;
    for (;;) {
        char candidate[PATH_MAX + 8];
        snprintf(candidate, sizeof(candidate), "%s/.env", dir);
        if (access(candidate, F_OK) == 0) { load_dotenv_file(candidate); return; }
        char *slash = strrchr(dir, '/');
        if (!slash || slash == dir) break;
        *slash = '\0';
    }
}

#define DATA_LIMIT 6705

// ---------------------------------------------------------------------------
// Stock data
// ---------------------------------------------------------------------------

typedef struct {
    char symbol[32];
    double *open, *high, *low, *close, *volume;
    size_t len;
} Stock;

// Loads stock OHLCV history directly from the Postgres `stocks` database via libpq.
static Stock load_from_db(PGconn *conn, const char *code, const char *exchange) {
    // Query used by scripts/fetch_data.sh (now replaced by this function)
    static const char QUERY_FMT[] =
        "SELECT e.open, e.high, e.low, e.close, e.volume "
        "FROM listing l "
        "INNER JOIN adj_eod e ON l.listing_id = e.listing_id "
        "WHERE l.code = '%s' AND l.exchange_code = '%s' AND e.volume > 0 "
        "ORDER BY e.ts ASC "
        "LIMIT %d";

    char query[1024];
    snprintf(query, sizeof(query), QUERY_FMT, code, exchange, DATA_LIMIT);

    PGresult *res = PQexec(conn, query);
    if (!res) {
        fprintf(stderr, "[error] PQexec failed for %s/%s: %s\n", code, exchange, PQerrorMessage(conn));
        exit(1);
    }

    if (PQresultStatus(res) != PGRES_TUPLES_OK) {
        fprintf(stderr, "[error] query for %s/%s returned status %d: %s\n",
                code, exchange, PQresultStatus(res), PQresultErrorMessage(res));
        PQclear(res);
        exit(1);
    }

    int nrows = PQntuples(res);
    if (nrows > DATA_LIMIT) {
        fprintf(stderr, "[error] query for %s/%s returned %d rows, exceeding limit %d\n",
                code, exchange, nrows, DATA_LIMIT);
        PQclear(res);
        exit(1);
    }

    Stock s;
    snprintf(s.symbol, sizeof(s.symbol), "%s_%s", code, exchange);
    s.open = malloc(sizeof(double) * DATA_LIMIT);
    s.high = malloc(sizeof(double) * DATA_LIMIT);
    s.low = malloc(sizeof(double) * DATA_LIMIT);
    s.close = malloc(sizeof(double) * DATA_LIMIT);
    s.volume = malloc(sizeof(double) * DATA_LIMIT);
    s.len = 0;

    // Columns: open(0), high(1), low(2), close(3), volume(4)
    for (int row = 0; row < nrows; row++) {
        const char *o_str = PQgetvalue(res, row, 0);
        const char *h_str = PQgetvalue(res, row, 1);
        const char *l_str = PQgetvalue(res, row, 2);
        const char *c_str = PQgetvalue(res, row, 3);
        const char *v_str = PQgetvalue(res, row, 4);

        s.open[row] = atof(o_str);
        s.high[row] = atof(h_str);
        s.low[row] = atof(l_str);
        s.close[row] = atof(c_str);
        s.volume[row] = atof(v_str);
        s.len++;
    }

    PQclear(res);

    printf("  loaded %zu bars  %s (from Postgres)\n", s.len, s.symbol);
    return s;
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

typedef struct {
    long long mean_ns;
    long long stddev_ns;
    long long min_ns;
    long long max_ns;
    int sample_count;
} TimingResult;

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double) ts.tv_sec * 1e9 + (double) ts.tv_nsec;
}

// fn(ctx) performs one full call-and-destroy cycle of the indicator under test.
typedef void (*BenchFn)(void *ctx);

static TimingResult time_fn(BenchFn fn, void *ctx, int number, int repeat, int warmup) {
    for (int i = 0; i < warmup; i++) fn(ctx);

    double *samples_ns = calloc((size_t) repeat, sizeof(double));
    for (int r = 0; r < repeat; r++) {
        double start = now_ns();
        for (int n = 0; n < number; n++) fn(ctx);
        double end = now_ns();
        samples_ns[r] = (end - start) / number;
    }

    double sum = 0.0, min = samples_ns[0], max = samples_ns[0];
    for (int r = 0; r < repeat; r++) {
        sum += samples_ns[r];
        if (samples_ns[r] < min) min = samples_ns[r];
        if (samples_ns[r] > max) max = samples_ns[r];
    }
    double mean = sum / repeat;

    double var = 0.0;
    if (repeat > 1) {
        for (int r = 0; r < repeat; r++) {
            double d = samples_ns[r] - mean;
            var += d * d;
        }
        var /= (repeat - 1); // sample stddev, matches Python's statistics.stdev
    }
    double stddev = sqrt(var);

    free(samples_ns);

    TimingResult res = {
        .mean_ns = (long long) mean,
        .stddev_ns = (long long) stddev,
        .min_ns = (long long) min,
        .max_ns = (long long) max,
        .sample_count = repeat,
    };
    return res;
}

// ---------------------------------------------------------------------------
// Result collection + reporting
// ---------------------------------------------------------------------------

typedef struct {
    char indicator[16];
    char impl_type[24];
    char stock_symbol[32];
    double options[8];
    int num_options;
    TimingResult timing;
    int input_size;
} BenchRow;

// Dynamically growing array of result rows. With 94 indicators x up to 3
// implementations x 4 stocks x 4 option sets, the row count can exceed 4000,
// so a fixed-size array is not viable -- grow it via realloc as needed.
static BenchRow *g_rows = NULL;
static int g_row_count = 0;
static int g_row_capacity = 0;

static void record_row(const char *indicator, const char *impl_type, const char *symbol, const double *options,
                        int num_options, TimingResult timing, int input_size) {
    if (g_row_count >= g_row_capacity) {
        g_row_capacity = g_row_capacity == 0 ? 1024 : g_row_capacity * 2;
        g_rows = realloc(g_rows, sizeof(BenchRow) * (size_t) g_row_capacity);
        if (!g_rows) { fprintf(stderr, "[error] failed to grow g_rows to %d entries\n", g_row_capacity); exit(1); }
    }
    BenchRow *row = &g_rows[g_row_count++];
    snprintf(row->indicator, sizeof(row->indicator), "%s", indicator);
    snprintf(row->impl_type, sizeof(row->impl_type), "%s", impl_type);
    snprintf(row->stock_symbol, sizeof(row->stock_symbol), "%s", symbol);
    memcpy(row->options, options, sizeof(double) * num_options);
    row->num_options = num_options;
    row->timing = timing;
    row->input_size = input_size;
}

static void print_row(const BenchRow *row) {
    char opts[128] = {0};
    int pos = 0;
    pos += snprintf(opts + pos, sizeof(opts) - pos, "[");
    for (int i = 0; i < row->num_options; i++) {
        pos += snprintf(opts + pos, sizeof(opts) - pos, "%s%.0f", i ? "," : "", row->options[i]);
    }
    snprintf(opts + pos, sizeof(opts) - pos, "]");

    printf("    %-8s %-20s %-10s %-16s %10.1f us +/- %.1f\n", row->indicator, row->impl_type, row->stock_symbol,
           opts, row->timing.mean_ns / 1000.0, row->timing.stddev_ns / 1000.0);
}

// Convenience used by every per-indicator driver in bench_indicators/: records a
// row and immediately prints it, so each driver only needs one call site per
// (indicator, implementation, stock, option-set) combination timed.
static void log_and_print(const char *indicator, const char *impl_type, const char *symbol, const double *options,
                           int num_options, TimingResult timing, int input_size) {
    record_row(indicator, impl_type, symbol, options, num_options, timing, input_size);
    print_row(&g_rows[g_row_count - 1]);
}

// ---------------------------------------------------------------------------
// DB logging -- uses direct libpq calls instead of popen("psql ...").
// All interpolated values are our own generated numeric/string literals
// (indicator names, fixed stock symbols, numeric option sets) — never external input.
// ---------------------------------------------------------------------------

static void log_results(const char *bench_db_url) {
    // Open connection to the benchmark results database
    PGconn *conn = PQconnectdb(bench_db_url);
    if (!conn || PQstatus(conn) != CONNECTION_OK) {
        fprintf(stderr, "[error] could not connect to benchmark DB: %s\n", conn ? PQerrorMessage(conn) : "PQconnectdb returned NULL");
        return;
    }

    // Insert into benchmark_runs and get the run_id
    PGresult *res = PQexec(conn,
        "INSERT INTO benchmark_runs (notes, system_info) VALUES "
        "('C FFI bindings benchmarks -- tulip_rs_ffi, C_tulip, talib', "
        "'{\"os\":\"linux\",\"binding\":\"ffi_c\"}'::jsonb) "
        "RETURNING id AS run_id");

    if (!res || PQresultStatus(res) != PGRES_TUPLES_OK) {
        fprintf(stderr, "[error] INSERT INTO benchmark_runs failed: %s\n",
                res ? PQresultErrorMessage(res) : "PQexec returned NULL");
        PQclear(res);
        PQfinish(conn);
        return;
    }

    char *run_id_str = PQgetvalue(res, 0, 0);
    long long run_id = strtoll(run_id_str, NULL, 10);
    PQclear(res);

    printf("  Logged benchmark_run with id=%lld\n", run_id);

    // Insert each benchmark result
    int inserted_count = 0;
    int failed_count = 0;

    for (int i = 0; i < g_row_count; i++) {
        const BenchRow *row = &g_rows[i];
        char opts[128] = {0};
        int pos = 0;
        pos += snprintf(opts + pos, sizeof(opts) - pos, "[");
        for (int j = 0; j < row->num_options; j++) {
            pos += snprintf(opts + pos, sizeof(opts) - pos, "%s%.1f", j ? "," : "", row->options[j]);
        }
        snprintf(opts + pos, sizeof(opts) - pos, "]");

        char insert_query[1024];
        snprintf(insert_query, sizeof(insert_query),
                "INSERT INTO benchmark_results "
                "(run_id, indicator_id, implementation_type, stock_symbol, data_source, options, "
                " mean_time_ns, std_dev_ns, min_time_ns, max_time_ns, sample_count, input_size) "
                "SELECT %lld, id, '%s', '%s', 'real_data', '%s'::jsonb, "
                "%lld, %lld, %lld, %lld, %d, %d "
                "FROM indicators WHERE name = '%s';",
                run_id, row->impl_type, row->stock_symbol, opts,
                row->timing.mean_ns, row->timing.stddev_ns,
                row->timing.min_ns, row->timing.max_ns, row->timing.sample_count, row->input_size, row->indicator);

        res = PQexec(conn, insert_query);
        if (!res || PQresultStatus(res) != PGRES_COMMAND_OK) {
            fprintf(stderr, "[warn] INSERT for %s/%s/%s failed: %s\n",
                    row->indicator, row->impl_type, row->stock_symbol,
                    res ? PQresultErrorMessage(res) : "PQexec returned NULL");
            PQclear(res);
            failed_count++;
        } else {
            PQclear(res);
            inserted_count++;
        }
    }

    printf("  Logged %d results; %d failures\n", inserted_count, failed_count);

    PQfinish(conn);
}

// ---------------------------------------------------------------------------
// Benchmark drivers -- one file per indicator under bench_indicators/, each
// defining bench_<name>/bench_tulipc_<name>/bench_talib_<name> + a
// run_<name>() driver. Included (not compiled separately) so they all share
// the Stock/TimingResult/log helpers above without extra translation units.
// ---------------------------------------------------------------------------

#include "bench_indicators/ad.c"
#include "bench_indicators/adaptivemsw.c"
#include "bench_indicators/adosc.c"
#include "bench_indicators/adx.c"
#include "bench_indicators/adxr.c"
#include "bench_indicators/ao.c"
#include "bench_indicators/apo.c"
#include "bench_indicators/aroon.c"
#include "bench_indicators/aroonosc.c"
#include "bench_indicators/atr.c"
#include "bench_indicators/avgprice.c"
#include "bench_indicators/bbands.c"
#include "bench_indicators/bop.c"
#include "bench_indicators/ccfisher.c"
#include "bench_indicators/cci.c"
#include "bench_indicators/chaikinmf.c"
#include "bench_indicators/chandelierexit.c"
#include "bench_indicators/cmo.c"
#include "bench_indicators/cvi.c"
#include "bench_indicators/cybercycle.c"
#include "bench_indicators/dema.c"
#include "bench_indicators/di.c"
#include "bench_indicators/dm.c"
#include "bench_indicators/donchianchannel.c"
#include "bench_indicators/dpo.c"
#include "bench_indicators/dx.c"
#include "bench_indicators/ef.c"
#include "bench_indicators/elderray.c"
#include "bench_indicators/ema.c"
#include "bench_indicators/emv.c"
#include "bench_indicators/fisher.c"
#include "bench_indicators/fosc.c"
#include "bench_indicators/highpass.c"
#include "bench_indicators/hilberttransform.c"
#include "bench_indicators/hma.c"
#include "bench_indicators/homodynediscriminator.c"
#include "bench_indicators/ichimoku.c"
#include "bench_indicators/instantaneoustrendline.c"
#include "bench_indicators/kama.c"
#include "bench_indicators/keltnerchannel.c"
#include "bench_indicators/kvo.c"
#include "bench_indicators/linreg.c"
#include "bench_indicators/macd.c"
#include "bench_indicators/mama.c"
#include "bench_indicators/marketfi.c"
#include "bench_indicators/mass.c"
#include "bench_indicators/max.c"
#include "bench_indicators/md.c"
#include "bench_indicators/medprice.c"
#include "bench_indicators/mfi.c"
#include "bench_indicators/min.c"
#include "bench_indicators/mom.c"
#include "bench_indicators/msw.c"
#include "bench_indicators/natr.c"
#include "bench_indicators/nvi.c"
#include "bench_indicators/obv.c"
#include "bench_indicators/pivotpoint.c"
#include "bench_indicators/ppo.c"
#include "bench_indicators/psar.c"
#include "bench_indicators/pvi.c"
#include "bench_indicators/qstick.c"
#include "bench_indicators/roc.c"
#include "bench_indicators/rocr.c"
#include "bench_indicators/roofingfilter.c"
#include "bench_indicators/rsi.c"
#include "bench_indicators/sma.c"
#include "bench_indicators/smaenvelope.c"
#include "bench_indicators/stddev.c"
#include "bench_indicators/stoch.c"
#include "bench_indicators/stochrsi.c"
#include "bench_indicators/supersmoother.c"
#include "bench_indicators/supertrend.c"
#include "bench_indicators/tema.c"
#include "bench_indicators/tr.c"
#include "bench_indicators/trendmode.c"
#include "bench_indicators/trima.c"
#include "bench_indicators/trix.c"
#include "bench_indicators/trvi.c"
#include "bench_indicators/tsf.c"
#include "bench_indicators/typprice.c"
#include "bench_indicators/ultosc.c"
#include "bench_indicators/vhf.c"
#include "bench_indicators/vidya.c"
#include "bench_indicators/volatility.c"
#include "bench_indicators/vortex.c"
#include "bench_indicators/vosc.c"
#include "bench_indicators/vwap.c"
#include "bench_indicators/vwma.c"
#include "bench_indicators/wad.c"
#include "bench_indicators/wcprice.c"
#include "bench_indicators/wilders.c"
#include "bench_indicators/willr.c"
#include "bench_indicators/wma.c"
#include "bench_indicators/zlema.c"

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

int main(void) {
    load_dotenv();
    TA_Initialize();

    int number = env_int("BENCH_NUMBER", 500);
    int repeat = env_int("BENCH_REPEAT", 10);
    int warmup = env_int("BENCH_WARMUP", 500);
    bool log_to_db = strcmp(env_str("BENCHMARK_LOG_TO_DB", "0"), "1") == 0;
    const char *stocks_db_url =
        env_str("DATABASE_URL", "postgres://tulip:tulip@localhost:5432/stocks");
    const char *bench_db_url =
        env_str("BENCHMARK_DATABASE_URL", "postgres://tulip:tulip@localhost:5432/indicator_benchmark");

    printf("================================================================\n");
    printf("  tulip_rs_ffi C Benchmark Suite\n");
    printf("================================================================\n");

    // Connect to the stocks database and load all 4 stocks
    printf("\n[1/2] Loading stock data from Postgres...\n");

    PGconn *stocks_conn = PQconnectdb(stocks_db_url);
    if (!stocks_conn || PQstatus(stocks_conn) != CONNECTION_OK) {
        fprintf(stderr, "[error] could not connect to stocks DB: %s\n",
                stocks_conn ? PQerrorMessage(stocks_conn) : "PQconnectdb returned NULL");
        exit(1);
    }

    Stock stocks[4] = {
        load_from_db(stocks_conn, "BHP", "ASX"),
        load_from_db(stocks_conn, "CBA", "ASX"),
        load_from_db(stocks_conn, "AAPL", "NYSE"),
        load_from_db(stocks_conn, "MSFT", "NYSE"),
    };

    PQfinish(stocks_conn);

    printf("\n[2/2] Running benchmarks (number=%d repeat=%d warmup=%d)...\n", number, repeat, warmup);
    run_ad(stocks, 4, number, repeat, warmup);
    run_adaptivemsw(stocks, 4, number, repeat, warmup);
    run_adosc(stocks, 4, number, repeat, warmup);
    run_adx(stocks, 4, number, repeat, warmup);
    run_adxr(stocks, 4, number, repeat, warmup);
    run_ao(stocks, 4, number, repeat, warmup);
    run_apo(stocks, 4, number, repeat, warmup);
    run_aroon(stocks, 4, number, repeat, warmup);
    run_aroonosc(stocks, 4, number, repeat, warmup);
    run_atr(stocks, 4, number, repeat, warmup);
    run_avgprice(stocks, 4, number, repeat, warmup);
    run_bbands(stocks, 4, number, repeat, warmup);
    run_bop(stocks, 4, number, repeat, warmup);
    run_ccfisher(stocks, 4, number, repeat, warmup);
    run_cci(stocks, 4, number, repeat, warmup);
    run_chaikinmf(stocks, 4, number, repeat, warmup);
    run_chandelierexit(stocks, 4, number, repeat, warmup);
    run_cmo(stocks, 4, number, repeat, warmup);
    run_cvi(stocks, 4, number, repeat, warmup);
    run_cybercycle(stocks, 4, number, repeat, warmup);
    run_dema(stocks, 4, number, repeat, warmup);
    run_di(stocks, 4, number, repeat, warmup);
    run_dm(stocks, 4, number, repeat, warmup);
    run_donchianchannel(stocks, 4, number, repeat, warmup);
    run_dpo(stocks, 4, number, repeat, warmup);
    run_dx(stocks, 4, number, repeat, warmup);
    run_ef(stocks, 4, number, repeat, warmup);
    run_elderray(stocks, 4, number, repeat, warmup);
    run_ema(stocks, 4, number, repeat, warmup);
    run_emv(stocks, 4, number, repeat, warmup);
    run_fisher(stocks, 4, number, repeat, warmup);
    run_fosc(stocks, 4, number, repeat, warmup);
    run_highpass(stocks, 4, number, repeat, warmup);
    run_hilberttransform(stocks, 4, number, repeat, warmup);
    run_hma(stocks, 4, number, repeat, warmup);
    run_homodynediscriminator(stocks, 4, number, repeat, warmup);
    run_ichimoku(stocks, 4, number, repeat, warmup);
    run_instantaneoustrendline(stocks, 4, number, repeat, warmup);
    run_kama(stocks, 4, number, repeat, warmup);
    run_keltnerchannel(stocks, 4, number, repeat, warmup);
    run_kvo(stocks, 4, number, repeat, warmup);
    run_linreg(stocks, 4, number, repeat, warmup);
    run_macd(stocks, 4, number, repeat, warmup);
    run_mama(stocks, 4, number, repeat, warmup);
    run_marketfi(stocks, 4, number, repeat, warmup);
    run_mass(stocks, 4, number, repeat, warmup);
    run_max(stocks, 4, number, repeat, warmup);
    run_md(stocks, 4, number, repeat, warmup);
    run_medprice(stocks, 4, number, repeat, warmup);
    run_mfi(stocks, 4, number, repeat, warmup);
    run_min(stocks, 4, number, repeat, warmup);
    run_mom(stocks, 4, number, repeat, warmup);
    run_msw(stocks, 4, number, repeat, warmup);
    run_natr(stocks, 4, number, repeat, warmup);
    run_nvi(stocks, 4, number, repeat, warmup);
    run_obv(stocks, 4, number, repeat, warmup);
    run_pivotpoint(stocks, 4, number, repeat, warmup);
    run_ppo(stocks, 4, number, repeat, warmup);
    run_psar(stocks, 4, number, repeat, warmup);
    run_pvi(stocks, 4, number, repeat, warmup);
    run_qstick(stocks, 4, number, repeat, warmup);
    run_roc(stocks, 4, number, repeat, warmup);
    run_rocr(stocks, 4, number, repeat, warmup);
    run_roofingfilter(stocks, 4, number, repeat, warmup);
    run_rsi(stocks, 4, number, repeat, warmup);
    run_sma(stocks, 4, number, repeat, warmup);
    run_smaenvelope(stocks, 4, number, repeat, warmup);
    run_stddev(stocks, 4, number, repeat, warmup);
    run_stoch(stocks, 4, number, repeat, warmup);
    run_stochrsi(stocks, 4, number, repeat, warmup);
    run_supersmoother(stocks, 4, number, repeat, warmup);
    run_supertrend(stocks, 4, number, repeat, warmup);
    run_tema(stocks, 4, number, repeat, warmup);
    run_tr(stocks, 4, number, repeat, warmup);
    run_trendmode(stocks, 4, number, repeat, warmup);
    run_trima(stocks, 4, number, repeat, warmup);
    run_trix(stocks, 4, number, repeat, warmup);
    run_trvi(stocks, 4, number, repeat, warmup);
    run_tsf(stocks, 4, number, repeat, warmup);
    run_typprice(stocks, 4, number, repeat, warmup);
    run_ultosc(stocks, 4, number, repeat, warmup);
    run_vhf(stocks, 4, number, repeat, warmup);
    run_vidya(stocks, 4, number, repeat, warmup);
    run_volatility(stocks, 4, number, repeat, warmup);
    run_vortex(stocks, 4, number, repeat, warmup);
    run_vosc(stocks, 4, number, repeat, warmup);
    run_vwap(stocks, 4, number, repeat, warmup);
    run_vwma(stocks, 4, number, repeat, warmup);
    run_wad(stocks, 4, number, repeat, warmup);
    run_wcprice(stocks, 4, number, repeat, warmup);
    run_wilders(stocks, 4, number, repeat, warmup);
    run_willr(stocks, 4, number, repeat, warmup);
    run_wma(stocks, 4, number, repeat, warmup);
    run_zlema(stocks, 4, number, repeat, warmup);

    printf("\n================================================================\n");
    printf("  Collected %d result rows\n", g_row_count);

    if (log_to_db) {
        printf("  Logging to %s ...\n", bench_db_url);
        log_results(bench_db_url);
    } else {
        printf("  BENCHMARK_LOG_TO_DB != 1 -- stdout only, not writing to DB\n");
    }
    printf("================================================================\n");

    for (int i = 0; i < 4; i++) {
        free(stocks[i].open);
        free(stocks[i].high);
        free(stocks[i].low);
        free(stocks[i].close);
        free(stocks[i].volume);
    }
    TA_Shutdown();
    return 0;
}
