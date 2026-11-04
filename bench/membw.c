#define _GNU_SOURCE
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>

#define MAXTH 64
#define MAXREP 64

typedef struct {
    int idx;
    int cpu;
    size_t n;
    int reps;
    double *a;
    double *b;
    double *c;
    double t0[2][MAXREP];
    double t1[2][MAXREP];
    double soma;
} Th;

static pthread_barrier_t barreira;

static double agora(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec * 1e-9;
}

__attribute__((noinline)) static void copia(double *restrict c, const double *restrict a, size_t n) {
    for (size_t i = 0; i < n; i++) c[i] = a[i];
}

__attribute__((noinline)) static void triad(double *restrict a, const double *restrict b, const double *restrict c, double s, size_t n) {
    for (size_t i = 0; i < n; i++) a[i] = b[i] + s * c[i];
}

static double *aloca(size_t bytes) {
    void *p = mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) {
        perror("mmap");
        exit(2);
    }
    return (double *)p;
}

static void *trabalho(void *arg) {
    Th *t = (Th *)arg;
    cpu_set_t cs;
    CPU_ZERO(&cs);
    CPU_SET(t->cpu, &cs);
    pthread_setaffinity_np(pthread_self(), sizeof(cs), &cs);
    size_t bytes = t->n * sizeof(double);
    t->a = aloca(bytes);
    t->b = aloca(bytes);
    t->c = aloca(bytes);
    for (size_t i = 0; i < t->n; i++) {
        t->a[i] = 1.0;
        t->b[i] = 2.0;
        t->c[i] = 0.5;
    }
    for (int k = 0; k < 2; k++) {
        for (int r = -1; r < t->reps; r++) {
            pthread_barrier_wait(&barreira);
            double s0 = agora();
            if (k == 0) copia(t->c, t->a, t->n);
            else triad(t->a, t->b, t->c, 3.0, t->n);
            double s1 = agora();
            if (r >= 0) {
                t->t0[k][r] = s0;
                t->t1[k][r] = s1;
            }
            if (k == 0) t->a[r + 1 < (int)t->n ? r + 1 : 0] += 1.0;
        }
    }
    double acc = 0;
    for (size_t i = 0; i < t->n; i += 4099) acc += t->a[i] + t->c[i];
    t->soma = acc;
    munmap(t->a, bytes);
    munmap(t->b, bytes);
    munmap(t->c, bytes);
    return NULL;
}

static int cmp(const void *x, const void *y) {
    double a = *(const double *)x, b = *(const double *)y;
    return (a > b) - (a < b);
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "uso: membw MIB_POR_VETOR_POR_THREAD REPETICOES CPU,CPU,... ROTULO\n");
        return 1;
    }
    size_t mib = strtoul(argv[1], NULL, 10);
    int reps = atoi(argv[2]);
    if (reps < 1 || reps > MAXREP) return 1;
    static Th th[MAXTH];
    static pthread_t id[MAXTH];
    int nth = 0;
    char *lista = strdup(argv[3]);
    for (char *tok = strtok(lista, ","); tok && nth < MAXTH; tok = strtok(NULL, ",")) {
        th[nth].idx = nth;
        th[nth].cpu = atoi(tok);
        th[nth].n = mib * 1048576 / sizeof(double);
        th[nth].reps = reps;
        nth++;
    }
    pthread_barrier_init(&barreira, NULL, nth);
    for (int i = 0; i < nth; i++) pthread_create(&id[i], NULL, trabalho, &th[i]);
    for (int i = 0; i < nth; i++) pthread_join(id[i], NULL);
    double soma = 0;
    for (int i = 0; i < nth; i++) soma += th[i].soma;
    const char *nomes[2] = {"copy", "triad"};
    double bytes_por_elem[2] = {16.0, 24.0};
    printf("{\"rotulo\":\"%s\",\"threads\":%d,\"mib_por_vetor_por_thread\":%zu,\"repeticoes\":%d,\"checksum\":%.3f,\"nucleos\":[", argv[4], nth, mib, reps, soma);
    for (int i = 0; i < nth; i++) printf("%s%d", i ? "," : "", th[i].cpu);
    printf("],\"kernels\":{");
    for (int k = 0; k < 2; k++) {
        double v[MAXREP];
        double melhor = 0;
        for (int r = 0; r < reps; r++) {
            double ini = th[0].t0[k][r], fim = th[0].t1[k][r];
            for (int i = 1; i < nth; i++) {
                if (th[i].t0[k][r] < ini) ini = th[i].t0[k][r];
                if (th[i].t1[k][r] > fim) fim = th[i].t1[k][r];
            }
            double total = bytes_por_elem[k] * (double)th[0].n * nth;
            v[r] = total / (fim - ini) / 1e9;
            if (v[r] > melhor) melhor = v[r];
        }
        double ord[MAXREP];
        memcpy(ord, v, sizeof(double) * reps);
        qsort(ord, reps, sizeof(double), cmp);
        double med = (reps % 2) ? ord[reps / 2] : 0.5 * (ord[reps / 2 - 1] + ord[reps / 2]);
        printf("%s\"%s\":{\"melhor_GBps\":%.3f,\"mediana_GBps\":%.3f,\"por_repeticao_GBps\":[", k ? "," : "", nomes[k], melhor, med);
        for (int r = 0; r < reps; r++) printf("%s%.3f", r ? "," : "", v[r]);
        printf("]}");
    }
    printf("}}\n");
    return 0;
}
