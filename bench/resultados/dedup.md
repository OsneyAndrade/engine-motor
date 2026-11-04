# dedup: F1, dedup por bloco (CDC) contra arquivo inteiro

Gerado por `bench/dedup.py relatorio` a partir de `resultados/dedup.json`. Hash: blake2b-256 no lugar de BLAKE3. Razao = (logico v1 + logico v2) / armazenado. Ganho extra = (armazenado baseline - armazenado CDC) / armazenado baseline, ambos com zstd-3.

## Testes de sanidade (TODOS APROVADOS)

| chunker | media / alvo (256 MiB aleatorios) | min | max | (a) | reaparecem apos inserir 100 B (64 MiB) | (b) | (c) round-trip | determinismo |
|---|---|---|---|---|---|---|---|---|
| fixo_4K | 1.000 | 4096 | 4096 | ok | 0.0% (0/16384) | ok | ok | ok |
| fixo_8K | 1.000 | 8192 | 8192 | ok | 0.0% (0/8192) | ok | ok | ok |
| fixo_64K | 1.000 | 65536 | 65536 | ok | 0.0% (0/1024) | ok | ok | ok |
| fixo_1M | 1.000 | 1048576 | 1048576 | ok | 0.0% (0/64) | ok | ok | ok |
| cdc_8K | 1.142 | 1464 | 29687 | ok | 99.9% (7165/7172) | ok | ok | ok |
| cdc_64K | 1.148 | 1464 | 210435 | ok | 99.9% (894/895) | ok | ok | ok |
| cdc_1M | 1.169 | 274417 | 2343654 | ok | 98.1% (53/54) | ok | ok | ok |

## Dados

| versao | xz (bytes) | sha256 confere | arquivos | vazios | dirs | symlinks | bytes de dados | bytes do tar |
|---|---|---|---|---|---|---|---|---|
| 6.12.115 | 148760708 | True | 86649 | 23 | 5761 | 62 | 1480853219 | 1550510080 |
| 6.12.114 | 148721296 | True | 86648 | 23 | 5761 | 62 | 1480791965 | 1550448640 |
| 6.13.12 | 148611164 | True | 87188 | 28 | 5796 | 62 | 1482692583 | 1552783360 |

SINTETICO: Silesia concatenado em ordem alfabetica; edicoes in-place 835 (2120332 B, 1.00%), insercoes 40 (44155 B), anexo 2119385 B (1.00%); backup1 211938580 B, backup2 214102120 B.

## Gate F1 (zstd-3, limite 5%)

| conjunto | estrategia | % v2 ja em v1 | razao total z3 | ganho extra sobre baseline | atende 5% | ganho c/ metadados | chunks unicos | indice 32 B | indice TB/mes @5PB | meses em 1 TB RAM | meses em 4 TB RAM |
|---|---|---|---|---|---|---|---|---|---|---|---|
| A_arvore | arquivo inteiro (baseline) | 99.17% | 10.415 | 0 (baseline) | | | 86325 | 2.76 MB | | | |
| A_arvore | fixo_4K | 99.50% | 7.629 | -36.53% | nao | -46.16% | 411702 | 13.17 MB | 22.242 | 0.04 | 0.18 |
| A_arvore | fixo_8K | 99.47% | 8.477 | -22.87% | nao | -27.13% | 236246 | 7.56 MB | 12.763 | 0.08 | 0.31 |
| A_arvore | fixo_64K | 99.28% | 9.762 | -6.69% | nao | -6.86% | 97244 | 3.11 MB | 5.254 | 0.19 | 0.76 |
| A_arvore | fixo_1M | 99.17% | 10.291 | -1.21% | nao | -1.19% | 86682 | 2.77 MB | 4.683 | 0.21 | 0.85 |
| A_arvore | cdc_8K | 99.79% | 9.053 | -15.05% | nao | -18.45% | 193357 | 6.19 MB | 10.446 | 0.10 | 0.38 |
| A_arvore | cdc_64K | 99.37% | 9.936 | -4.83% | nao | -4.96% | 94206 | 3.01 MB | 5.089 | 0.20 | 0.79 |
| A_arvore | cdc_1M | 99.17% | 10.343 | -0.70% | nao | -0.69% | 86541 | 2.77 MB | 4.675 | 0.21 | 0.86 |
| A_tar | zstd-3 sem dedup (baseline) | 0 | 6.960 | 0 (baseline) | | | | | | | |
| A_tar | fixo_4K | 45.00% | 4.717 | -47.54% | nao | -57.19% | 586278 | 18.76 MB | 30.250 | 0.03 | 0.13 |
| A_tar | fixo_8K | 11.36% | 4.817 | -44.50% | nao | -49.78% | 356907 | 11.42 MB | 18.415 | 0.05 | 0.22 |
| A_tar | fixo_64K | 10.41% | 5.967 | -16.65% | nao | -17.31% | 44853 | 1.44 MB | 2.314 | 0.43 | 1.73 |
| A_tar | fixo_1M | 10.21% | 7.121 | 2.25% | nao | 2.21% | 2807 | 89.82 KB | 0.145 | 6.90 | 27.62 |
| A_tar | cdc_8K | 99.58% | 9.562 | 27.21% | sim | 24.00% | 139623 | 4.47 MB | 7.204 | 0.14 | 0.56 |
| A_tar | cdc_64K | 98.38% | 11.254 | 38.15% | sim | 37.75% | 18573 | 594.34 KB | 0.958 | 1.04 | 4.17 |
| A_tar | cdc_1M | 89.66% | 11.719 | 40.60% | sim | 40.58% | 1247 | 39.90 KB | 0.064 | 15.54 | 62.17 |
| B_arvore | arquivo inteiro (baseline) | 66.23% | 7.299 | 0 (baseline) | | | 106405 | 3.40 MB | | | |
| B_arvore | fixo_4K | 74.80% | 5.806 | -25.72% | nao | -33.31% | 511318 | 16.36 MB | 27.606 | 0.04 | 0.14 |
| B_arvore | fixo_8K | 73.31% | 6.354 | -14.87% | nao | -18.31% | 294460 | 9.42 MB | 15.898 | 0.06 | 0.25 |
| B_arvore | fixo_64K | 67.32% | 6.964 | -4.81% | nao | -4.97% | 119587 | 3.83 MB | 6.456 | 0.15 | 0.62 |
| B_arvore | fixo_1M | 66.23% | 7.233 | -0.91% | nao | -0.90% | 106782 | 3.42 MB | 5.765 | 0.17 | 0.69 |
| B_arvore | cdc_8K | 81.10% | 7.164 | -1.88% | nao | -4.67% | 230421 | 7.37 MB | 12.440 | 0.08 | 0.32 |
| B_arvore | cdc_64K | 69.50% | 7.128 | -2.40% | nao | -2.55% | 115542 | 3.70 MB | 6.238 | 0.16 | 0.64 |
| B_arvore | cdc_1M | 67.21% | 7.271 | -0.38% | nao | -0.38% | 106625 | 3.41 MB | 5.757 | 0.17 | 0.69 |
| B_tar | zstd-3 sem dedup (baseline) | 0 | 6.965 | 0 (baseline) | | | | | | | |
| B_tar | fixo_4K | 15.35% | 4.219 | -65.08% | nao | -75.54% | 698945 | 22.37 MB | 36.036 | 0.03 | 0.11 |
| B_tar | fixo_8K | 11.81% | 4.645 | -49.95% | nao | -55.23% | 356322 | 11.40 MB | 18.371 | 0.05 | 0.22 |
| B_tar | fixo_64K | 0.11% | 5.597 | -24.44% | nao | -25.12% | 47328 | 1.51 MB | 2.440 | 0.41 | 1.64 |
| B_tar | fixo_1M | 0.00% | 6.678 | -4.31% | nao | -4.35% | 2960 | 94.72 KB | 0.153 | 6.55 | 26.21 |
| B_tar | cdc_8K | 71.74% | 6.891 | -1.08% | nao | -4.60% | 182355 | 5.84 MB | 9.402 | 0.11 | 0.43 |
| B_tar | cdc_64K | 48.04% | 6.795 | -2.51% | nao | -2.98% | 28392 | 908.54 KB | 1.464 | 0.68 | 2.73 |
| B_tar | cdc_1M | 31.36% | 7.111 | 2.06% | nao | 2.03% | 1966 | 62.91 KB | 0.101 | 9.87 | 39.46 |
| sintetico | zstd-3 sem dedup (baseline) | 0 | 3.111 | 0 (baseline) | | | | | | | |
| sintetico | fixo_4K | 0.93% | 2.332 | -33.41% | nao | -38.25% | 103430 | 3.31 MB | 38.843 | 0.03 | 0.10 |
| sintetico | fixo_8K | 0.77% | 2.494 | -24.72% | nao | -27.15% | 51804 | 1.66 MB | 19.455 | 0.05 | 0.21 |
| sintetico | fixo_64K | 0.70% | 2.831 | -9.87% | nao | -10.17% | 6478 | 207.30 KB | 2.433 | 0.41 | 1.64 |
| sintetico | fixo_1M | 0.00% | 3.071 | -1.28% | nao | -1.30% | 408 | 13.06 KB | 0.153 | 6.53 | 26.11 |
| sintetico | cdc_8K | 92.58% | 4.626 | 32.76% | sim | 31.03% | 25699 | 822.37 KB | 9.651 | 0.10 | 0.41 |
| sintetico | cdc_64K | 68.97% | 4.309 | 27.82% | sim | 27.60% | 3723 | 119.14 KB | 1.398 | 0.72 | 2.86 |
| sintetico | cdc_1M | 0.37% | 3.082 | -0.92% | nao | -0.93% | 350 | 11.20 KB | 0.131 | 7.61 | 30.43 |

## Armazenado total (v1+v2), bytes

| conjunto | estrategia | bruto | zstd-3 | zstd-19 (estimado por amostra) | razao bruta | razao z3 | razao z19 | % v2 duplicado (v1 e v2) | media chunk unico | bit-exato |
|---|---|---|---|---|---|---|---|---|---|---|
| A_arvore | arquivo inteiro sem dedup | 2961645184 | 563250174 | | 1.000 | 5.258 | | 0 | | True |
| A_arvore | arquivo inteiro com dedup | 1489650415 | 284353242 | | 1.988 | 10.415 | | 99.17% | | True |
| A_arvore | fixo_4K | 1481504795 | 388228327 |  | 1.999 | 7.629 |  | 99.50% | 3598 | True |
| A_arvore | fixo_8K | 1482695083 | 349372047 |  | 1.997 | 8.477 |  | 99.47% | 6276 | True |
| A_arvore | fixo_64K | 1487432368 | 303384649 |  | 1.991 | 9.762 |  | 99.28% | 15295 | True |
| A_arvore | fixo_1M | 1489650415 | 287798767 |  | 1.988 | 10.291 |  | 99.17% | 17185 | True |
| A_arvore | cdc_8K | 1330569335 | 327143950 |  | 2.226 | 9.053 |  | 99.79% | 6881 | True |
| A_arvore | cdc_64K | 1434382016 | 298080654 | 259059328 | 2.065 | 9.936 | 11.432 | 99.37% | 15226 | True |
| A_arvore | cdc_1M | 1485426020 | 286343231 | 247772452 | 1.994 | 10.343 | 11.953 | 99.17% | 17164 | True |
| A_tar | fluxo sem dedup | 3100958720 | 445513267 | | 1.000 | 6.960 | | 0 | | True |
| A_tar | EXTRA zstd-3 long=31 (v1+v2 concatenados) | 3100958720 | 206965655 | | | 14.983 | | | | True |
| A_tar | fixo_4K | 2401390592 | 657332177 |  | 1.291 | 4.717 |  | 45.03% | 4095 | True |
| A_tar | fixo_8K | 2923773952 | 643774444 |  | 1.061 | 4.817 |  | 11.39% | 8191 | True |
| A_tar | fixo_64K | 2939478016 | 519693284 |  | 1.055 | 5.967 |  | 10.41% | 65535 | True |
| A_tar | fixo_1M | 2942623744 | 435469140 |  | 1.054 | 7.121 |  | 10.21% | 1048316 | True |
| A_tar | cdc_8K | 1405505132 | 324301441 |  | 2.206 | 9.562 |  | 99.58% | 10066 | True |
| A_tar | cdc_64K | 1522924796 | 275536022 | 230546434 | 2.036 | 11.254 | 13.450 | 98.38% | 81996 | True |
| A_tar | cdc_1M | 1706563437 | 264618239 | 205109222 | 1.817 | 11.719 | 15.119 | 89.66% | 1368535 | True |
| B_arvore | arquivo inteiro sem dedup | 2963545802 | 563412899 | | 1.000 | 5.260 | | 0 | | True |
| B_arvore | arquivo inteiro com dedup | 1977520013 | 406026793 | | 1.499 | 7.299 | | 66.27% | | True |
| B_arvore | fixo_4K | 1847286343 | 510465525 |  | 1.604 | 5.806 |  | 74.83% | 3612 | True |
| B_arvore | fixo_8K | 1870116088 | 466404147 |  | 1.585 | 6.354 |  | 73.34% | 6351 | True |
| B_arvore | fixo_64K | 1960738011 | 425551089 |  | 1.511 | 6.964 |  | 67.36% | 16395 | True |
| B_arvore | fixo_1M | 1977520013 | 409718127 |  | 1.499 | 7.233 |  | 66.27% | 18519 | True |
| B_arvore | cdc_8K | 1607355109 | 413657945 |  | 1.844 | 7.164 |  | 81.13% | 6975 | True |
| B_arvore | cdc_64K | 1876864670 | 415772450 | 361652551 | 1.579 | 7.128 | 8.194 | 69.54% | 16244 | True |
| B_arvore | cdc_1M | 1958680152 | 407582055 | 352366403 | 1.513 | 7.271 | 8.410 | 67.25% | 18369 | True |
| B_tar | fluxo sem dedup | 3103293440 | 445538453 | | 1.000 | 6.965 | | 0 | | True |
| B_tar | EXTRA zstd-3 long=31 (v1+v2 concatenados) | 3103293440 | 213069478 | | | 14.565 | | | | True |
| B_tar | fixo_4K | 2862874624 | 735481578 |  | 1.084 | 4.219 |  | 15.41% | 4095 | True |
| B_tar | fixo_8K | 2918981632 | 668066986 |  | 1.063 | 4.645 |  | 11.84% | 8191 | True |
| B_tar | fixo_64K | 3101655040 | 554412125 |  | 1.001 | 5.597 |  | 0.11% | 65535 | True |
| B_tar | fixo_1M | 3103293440 | 464736986 |  | 1.000 | 6.678 |  | 0.00% | 1048409 | True |
| B_tar | cdc_8K | 1837790384 | 450367587 |  | 1.689 | 6.891 |  | 71.75% | 10078 | True |
| B_tar | cdc_64K | 2304622852 | 456715498 | 385108264 | 1.347 | 6.795 | 8.058 | 48.04% | 81171 | True |
| B_tar | cdc_1M | 2612038966 | 436379228 | 339114244 | 1.188 | 7.111 | 9.151 | 31.36% | 1328605 | True |
| sintetico | fluxo sem dedup | 426040700 | 136965128 | | 1.000 | 3.111 | | 0 | | True |
| sintetico | EXTRA zstd-3 long=31 (v1+v2 concatenados) | 426040700 | 69727540 | | | 6.110 | | | | True |
| sintetico | fixo_4K | 423644540 | 182719916 |  | 1.006 | 2.332 |  | 0.93% | 4095 | True |
| sintetico | fixo_8K | 424369532 | 170823936 |  | 1.004 | 2.494 |  | 0.78% | 8191 | True |
| sintetico | fixo_64K | 424533372 | 150479277 |  | 1.004 | 2.831 |  | 0.70% | 65534 | True |
| sintetico | fixo_1M | 426040700 | 138718425 |  | 1.000 | 3.071 |  | 0.00% | 1044217 | True |
| sintetico | cdc_8K | 227653861 | 92090720 |  | 1.871 | 4.626 |  | 92.58% | 8858 | True |
| sintetico | cdc_64K | 278378294 | 98862660 | 87325480 | 1.530 | 4.309 | 4.879 | 68.97% | 74772 | True |
| sintetico | cdc_1M | 425239725 | 138223041 | 117773429 | 1.002 | 3.082 | 3.617 | 0.37% | 1214970 | True |

## Delta por caminho (limite superior de dedup por similaridade)

| conjunto | arquivos alterados | bytes logicos alterados (v2) | zstd-3 sozinho | patch-from z3 | CDC 8K marginal | CDC 64K marginal | CDC 1M marginal | total arquivo inteiro z3 | total delta por caminho | total CDC 8K z3 | total CDC 64K z3 | razao delta | razao CDC 64K |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A_arvore | 223 | 12316019 | 3328136 | 42562 | 1038651 | 2624735 | 3328136 | 284353242 | 281067668 | 327143950 | 298080654 | 10.537 | 9.936 |
| B_arvore | 19227 | 492693123 | 123125774 | 3395051 | 85322997 | 118214546 | 122473136 | 406026793 | 286513981 | 413657945 | 415772450 | 10.343 | 7.128 |

Conferencia do prefixo de `compression.zstd` contra `zstd --patch-from` (CLI): A_arvore: 40 arquivos, diferenca relativa media 1.576%, max 13.483%; B_arvore: 40 arquivos, diferenca relativa media 6.856%, max 39.655%

## Vazao em 1 nucleo (CPU 0, mediana de 3, MB = 1e6 B)

Fonte: primeiros 256 MiB do tar de 6.12.115 (lookup preenchido com os primeiros 256 MiB do tar de 6.12.114). Hash: hashlib.blake2b(digest_size=32), SUBSTITUTO de BLAKE3 (import blake3 inexistente). Lookup: dict do Python (NAO representa indice de producao). blake2b em buffer unico: 1247 MB/s.

| chunker | chunks | media | chunker MB/s | hash por chunk MB/s | chunker+hash+lookup MB/s |
|---|---|---|---|---|---|
| fixo_4K | 65536 | 4096 | n/a (aritmetica de offsets) | 1162 | 1120 |
| fixo_8K | 32768 | 8192 | n/a (aritmetica de offsets) | 1198 | 1178 |
| fixo_64K | 4096 | 65536 | n/a (aritmetica de offsets) | 1235 | 1236 |
| fixo_1M | 256 | 1048576 | n/a (aritmetica de offsets) | 1247 | 1245 |
| cdc_8K | 27020 | 9934 | 3417 | 1198 | 883 |
| cdc_64K | 3341 | 80345 | 3468 | 1241 | 913 |
| cdc_1M | 195 | 1376592 | 3427 | 1248 | 915 |

## Bit-exatidao

Violacoes: nenhuma (reconstrucao de cada arquivo da arvore e de cada fluxo a partir de chunks unicos + receitas, sha256 igual ao original; round-trip zstd-3 de cada chunk unico).

