# escala em nucleos homogeneos (zstd-3 e zstd-1, blocos de 1 MiB, processos independentes)

Gerado por `bench/escala_resumo.py` a partir de `resultados/escala_homogenea.json`. Mesma metodologia da fase `escala` de `medicoes.json`: dados distintos por processo, barreira, rodadas de 1.5 s, 1 rodada de aquecimento descartada, mediana de 3 rodadas. CPU: Intel(R) Core(TM) i9-14900HX.

Eficiencia (vs mesmo tipo) = agregado medido / soma da vazao isolada de cada trabalhador medida em um nucleo do MESMO tipo (CPU 0 para P, CPU 16 para E). Eficiencia (vs P-core) = a definicao da curva anterior (todo trabalhador comparado ao P-core isolado).

## Vazao isolada (1 nucleo), pool inteiro

| config | P comp MB/s | E comp MB/s | E/P comp | P decomp MB/s | E decomp MB/s | E/P decomp |
|---|---|---|---|---|---|---|
| zstd-3_bloco_1MiB | 463 | 275 | 0.593 | 1904 | 1205 | 0.633 |
| zstd-1_bloco_1MiB | 665 | 411 | 0.618 | 1967 | 1300 | 0.661 |

## (a) so E-cores fisicos, zstd-3_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 16 | 299 | 299 | 1.01 | 0.60 | 1290 | 1290 | 1.01 | 0.62 |
| 2 | 16,17 | 536 | 268 | 0.97 | 0.57 | 2285 | 1143 | 0.93 | 0.58 |
| 4 | 16,17,18,19 | 806 | 202 | 0.74 | 0.44 | 4007 | 1002 | 0.84 | 0.53 |
| 8 | 16,17,18,19,20,21,22,23 | 1627 | 203 | 0.76 | 0.45 | 7862 | 983 | 0.83 | 0.52 |
| 12 | 16,17,18,19,20,21,22,23,24,25,26,27 | 2386 | 199 | 0.73 | 0.43 | 11542 | 962 | 0.80 | 0.50 |
| 16 | 16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31 | 3128 | 195 | 0.72 | 0.43 | 13890 | 868 | 0.72 | 0.46 |

## (a) so E-cores fisicos, zstd-1_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 16 | 436 | 436 | 1.00 | 0.61 | 1412 | 1412 | 1.04 | 0.68 |
| 2 | 16,17 | 819 | 410 | 0.98 | 0.60 | 2573 | 1287 | 0.97 | 0.65 |
| 4 | 16,17,18,19 | 1418 | 355 | 0.89 | 0.55 | 4500 | 1125 | 0.87 | 0.58 |
| 8 | 16,17,18,19,20,21,22,23 | 2767 | 346 | 0.90 | 0.56 | 8828 | 1104 | 0.87 | 0.57 |
| 12 | 16,17,18,19,20,21,22,23,24,25,26,27 | 4164 | 347 | 0.89 | 0.55 | 12944 | 1079 | 0.84 | 0.55 |
| 16 | 16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31 | 5536 | 346 | 0.88 | 0.54 | 15807 | 988 | 0.77 | 0.51 |

## (b) so P-cores fisicos, zstd-3_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 0 | 500 | 500 | 1.00 | 1.00 | 2071 | 2071 | 1.00 | 1.00 |
| 2 | 0,2 | 912 | 456 | 0.98 | 0.98 | 3843 | 1921 | 0.98 | 0.98 |
| 4 | 0,2,4,6 | 1651 | 413 | 0.91 | 0.91 | 6869 | 1717 | 0.90 | 0.90 |
| 8 | 0,2,4,6,8,10,12,14 | 3196 | 399 | 0.89 | 0.89 | 13257 | 1657 | 0.87 | 0.87 |

## (b) so P-cores fisicos, zstd-1_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 0 | 727 | 727 | 1.02 | 1.02 | 2125 | 2125 | 1.02 | 1.02 |
| 2 | 0,2 | 1360 | 680 | 0.99 | 0.99 | 4012 | 2006 | 1.01 | 1.01 |
| 4 | 0,2,4,6 | 2414 | 603 | 0.93 | 0.93 | 7151 | 1788 | 0.92 | 0.92 |
| 8 | 0,2,4,6,8,10,12,14 | 4544 | 568 | 0.91 | 0.91 | 13774 | 1722 | 0.89 | 0.89 |

## (c) P-cores com SMT, zstd-3_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 8 | 0,2,4,6,8,10,12,14 | 3122 | 390 | 0.87 | 0.87 | 13052 | 1631 | 0.86 | 0.86 |
| 16 | 0,2,4,6,8,10,12,14,1,3,5,7,9,11,13,15 | 3729 | 233 | 0.51 | 0.51 | 14639 | 915 | 0.48 | 0.48 |

## (c) P-cores com SMT, zstd-1_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 8 | 0,2,4,6,8,10,12,14 | 4452 | 556 | 0.89 | 0.89 | 13604 | 1700 | 0.88 | 0.88 |
| 16 | 0,2,4,6,8,10,12,14,1,3,5,7,9,11,13,15 | 5542 | 346 | 0.54 | 0.54 | 16395 | 1025 | 0.53 | 0.53 |

## (d) 8 P fisicos + 8 E, zstd-3_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 16 | 0,2,4,6,8,10,12,14,16,17,18,19,20,21,22,23 | 4310 | 269 | 0.74 | 0.59 | 19195 | 1200 | 0.77 | 0.63 |

## (d) 8 P fisicos + 8 E, zstd-1_bloco_1MiB

| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |
|---|---|---|---|---|---|---|---|---|---|
| 16 | 0,2,4,6,8,10,12,14,16,17,18,19,20,21,22,23 | 6769 | 423 | 0.83 | 0.66 | 20619 | 1289 | 0.79 | 0.66 |

## Derivados

### zstd-3_bloco_1MiB

- Ganho do SMT: 16 threads em 8 P-cores / 8 threads em 8 P-cores fisicos: comp 1.167x (3196 -> 3729 MB/s), decomp 1.104x (13257 -> 14639 MB/s).
- Repetibilidade: N=8 em 8 P fisicos medido duas vezes (grupos P e PSMT): comp 3196 vs 3122 MB/s, decomp 13257 vs 13052 MB/s.
- 16 E-cores homogeneos: eficiencia vs 1 E-core, comp 0.72, decomp 0.72.
- 8 P + 8 E (N=16 misto): eficiencia vs P-core (definicao anterior) comp 0.59, decomp 0.63; vs mesmo tipo comp 0.74, decomp 0.77.

### zstd-1_bloco_1MiB

- Ganho do SMT: 16 threads em 8 P-cores / 8 threads em 8 P-cores fisicos: comp 1.219x (4544 -> 5542 MB/s), decomp 1.190x (13774 -> 16395 MB/s).
- Repetibilidade: N=8 em 8 P fisicos medido duas vezes (grupos P e PSMT): comp 4544 vs 4452 MB/s, decomp 13774 vs 13604 MB/s.
- 16 E-cores homogeneos: eficiencia vs 1 E-core, comp 0.88, decomp 0.77.
- 8 P + 8 E (N=16 misto): eficiencia vs P-core (definicao anterior) comp 0.66, decomp 0.66; vs mesmo tipo comp 0.83, decomp 0.79.

## Curva anterior (medicoes.json, somente leitura)

| config | N | comp MB/s | ef comp | decomp MB/s | ef decomp |
|---|---|---|---|---|---|
| zstd-3_bloco_1MiB | 1 | 500 | 1.08 | 2049 | 1.06 |
| zstd-3_bloco_1MiB | 8 | 3170 | 0.95 | 13168 | 0.91 |
| zstd-3_bloco_1MiB | 16 | 4363 | 0.64 | 19331 | 0.67 |
| zstd-3_bloco_1MiB | 24 | 5351 | 0.52 | 19082 | 0.44 |
| zstd-3_bloco_1MiB | 32 | 5846 | 0.42 | 14275 | 0.25 |
| zstd-1_bloco_1MiB | 1 | 711 | 1.03 | 2117 | 1.04 |
| zstd-1_bloco_1MiB | 8 | 4466 | 0.91 | 13546 | 0.89 |
| zstd-1_bloco_1MiB | 16 | 6795 | 0.67 | 20677 | 0.68 |
| zstd-1_bloco_1MiB | 24 | 8727 | 0.57 | 21643 | 0.47 |
| zstd-1_bloco_1MiB | 32 | 10085 | 0.47 | 16594 | 0.27 |

