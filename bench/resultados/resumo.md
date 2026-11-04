# Resumo do benchmark Syntra (compressao sem perdas + economia)

Gerado por `economia.py` a partir de `medicoes.json` e `precos.json`. Data da execucao: 2026-11-03 15:37:48. Unidades: MB = 10^6 B, GB = 10^9 B.

## 1. Ambiente e protocolo

| Item | Valor |
|---|---|
| CPU | Intel(R) Core(TM) i9-14900HX |
| P-cores (threads) | 8 nucleos fisicos, CPUs 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15 |
| E-cores (threads) | 16 nucleos fisicos, CPUs 16-31 |
| Governor / no_turbo | powersave / 0 |
| Python / libzstd (modulo compression.zstd) | 3.14.4 / 1.5.7 |
| zstd CLI / xz CLI | *** Zstandard CLI (64-bit) v1.5.7, by Yann Collet *** / xz (XZ Utils) 5.8.3 |
| zlib / bzip2 | 1.3.1 / bzip2, a block-sorting file compressor.  Version 1.0.8, 13-Jul-2019. |
| Repeticoes (mediana) | 3 |
| Fatia maxima por arquivo | 8 MiB |
| P-cores da medicao principal (4 trabalhadores concorrentes) | 0,2,4,6 |
| Energia (RAPL) | energia nao medida: energy_uj sem permissao de leitura sem sudo ([Errno 13] Permission denied: '/sys/class/powercap/intel-rapl-mmio:0/energy_uj') |
| Violacoes de bit-exatidao | 0 |
| Celulas com falha de execucao | 0 |


### Arquivos usados (fatia medida, sha256 do arquivo inteiro)

| Arquivo | Classe | Sintetico | Tamanho | Fatia | sha256 (arquivo) |
|---|---|---|---|---|---|
| dickens | texto | nao | 10192446 | 8388608 | b24c37886142e11d... |
| mozilla | binario | nao | 51220480 | 8388608 | 657fc3764b0c75ac... |
| mr | cientifico | nao | 9970564 | 8388608 | 68637ed52e3e4860... |
| nci | cientifico | nao | 33553445 | 8388608 | fc63a31770947b8c... |
| ooffice | binario | nao | 6152192 | 6152192 | e7ee013880d34dd5... |
| osdb | banco_de_dados | nao | 10085684 | 8388608 | 60f027179302ca3a... |
| reymont | texto | nao | 6627202 | 6627202 | 0eac0114a3dfe6e2... |
| samba | codigo | nao | 21606400 | 8388608 | 93ba07bc44d82677... |
| sao | cientifico | nao | 7251944 | 7251944 | c2d0ea2cc59d4c21... |
| webster | texto | nao | 41458703 | 8388608 | 6a68f69b26daf09f... |
| x-ray | cientifico | nao | 8474240 | 8388608 | 7de9fce1405dc44a... |
| xml | xml | nao | 5345280 | 5345280 | 0e82e54e695c1938... |
| bible.txt | texto | nao | 4047392 | 4047392 | 4e0a7e8dff7d9c82... |
| E.coli | dna | nao | 4638690 | 4638690 | 9125dfd87315961e... |
| world192.txt | texto | nao | 2473400 | 2473400 | 1aebdc97d29904b2... |
| aleatorio.bin | sint_incompressivel | SIM | 33554432 | 8388608 | d6049af107717c5f... |
| logs.log | sint_logs | SIM | 67108864 | 8388608 | 6674e2b8fe54f124... |
| ja_comprimido.xz | sint_ja_comprimido | SIM | 8385868 | 8385868 | 872fb8ea4ebddf9e... |


Sha256 completos em `medicoes.json` (campo `arquivos`).

## 2. Requisito

5 PB/mes: media rho = 1.929 MB/s (1,929 GB/s); pico k=4: 7.716 MB/s (7,72 GB/s). Retencao T=60 meses; fator de vazao servidor/notebook = 1,00.

Teste de validacao a mao: PASSOU (detalhes em `economia.json`, campo `autoteste`).

## 3. Escala multi-nucleo (processos independentes, dados distintos, blocos de 1 MiB)

Ordem de fixacao: P-cores fisicos (CPUs 0,2,...,14), depois E-cores (16..31), depois irmaos SMT dos P-cores (1,3,...,15). Eficiencia = agregado / soma das vazoes isoladas de 1 nucleo (P-core, CPU 0) sobre os mesmos dados.


**zstd-3_bloco_1MiB** (duracao 1.5 s por rodada, mediana de 3 rodadas)

| N | comp agregada MB/s | comp por nucleo MB/s | comp eficiencia | decomp agregada MB/s | decomp eficiencia | bit-exato |
|---|---|---|---|---|---|---|
| 1 | 500 | 500 | 1,08 | 2.049 | 1,06 | sim |
| 2 | 904 | 452 | 1,04 | 3.816 | 1,04 | sim |
| 4 | 1.659 | 415 | 0,98 | 6.869 | 0,97 | sim |
| 8 | 3.170 | 396 | 0,95 | 13.168 | 0,91 | sim |
| 16 | 4.363 | 273 | 0,64 | 19.331 | 0,67 | sim |
| 24 | 5.351 | 223 | 0,52 | 19.082 | 0,44 | sim |
| 32 | 5.846 | 183 | 0,42 | 14.275 | 0,25 | sim |


**zstd-1_bloco_1MiB** (duracao 1.5 s por rodada, mediana de 3 rodadas)

| N | comp agregada MB/s | comp por nucleo MB/s | comp eficiencia | decomp agregada MB/s | decomp eficiencia | bit-exato |
|---|---|---|---|---|---|---|
| 1 | 711 | 711 | 1,03 | 2.117 | 1,04 | sim |
| 2 | 1.332 | 666 | 0,99 | 3.940 | 1,01 | sim |
| 4 | 2.425 | 606 | 0,95 | 7.156 | 0,94 | sim |
| 8 | 4.466 | 558 | 0,91 | 13.546 | 0,89 | sim |
| 16 | 6.795 | 425 | 0,67 | 20.677 | 0,68 | sim |
| 24 | 8.727 | 364 | 0,57 | 21.643 | 0,47 | sim |
| 32 | 10.085 | 315 | 0,47 | 16.594 | 0,27 | sim |

## 4. Razao (x) por classe e configuracao


**Modo inteiro** (razao = original / comprimido)

| classe | stored | zlib-1 | zlib-6 | zlib-9 | zstd-1 | zstd-3 | zstd-3-long27 | zstd-9 | zstd-19 | zstd-22 | xz-1 | xz-6 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| texto | 1,00 | 2,58 | 3,17 | 3,21 | 2,86 | 3,24 | 3,25 | 3,71 | 4,32 | 4,32 | 3,37 | 4,37 | 4,37 | 4,51 |
| codigo | 1,00 | 2,50 | 2,80 | 2,81 | 2,85 | 3,18 | 3,24 | 3,49 | 3,72 | 3,72 | 3,26 | 3,75 | 3,77 | 3,24 |
| binario | 1,00 | 1,75 | 1,83 | 1,83 | 1,72 | 1,87 | 1,87 | 1,97 | 2,08 | 2,08 | 2,06 | 2,18 | 2,18 | 1,90 |
| cientifico | 1,00 | 2,04 | 2,13 | 2,14 | 1,92 | 2,13 | 2,14 | 2,33 | 2,47 | 2,47 | 2,49 | 2,79 | 2,80 | 2,83 |
| banco_de_dados | 1,00 | 2,47 | 2,73 | 2,75 | 2,70 | 2,88 | 2,88 | 3,01 | 3,24 | 3,24 | 2,95 | 3,53 | 3,53 | 3,60 |
| xml | 1,00 | 5,54 | 7,77 | 8,11 | 7,71 | 8,41 | 8,95 | 10,34 | 11,80 | 11,84 | 9,54 | 11,79 | 12,29 | 12,12 |
| dna | 1,00 | 3,04 | 3,46 | 3,57 | 3,29 | 3,33 | 3,35 | 3,47 | 4,03 | 4,03 | 3,37 | 3,91 | 3,91 | 3,71 |
| sint_logs | 1,00 | 3,26 | 4,09 | 4,26 | 4,10 | 4,08 | 4,07 | 4,59 | 5,72 | 5,72 | 4,14 | 5,76 | 5,76 | 6,24 |
| sint_incompressivel | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 |
| sint_ja_comprimido | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 |
| mix_real | 1,00 | 2,30 | 2,55 | 2,56 | 2,36 | 2,60 | 2,61 | 2,85 | 3,09 | 3,09 | 2,85 | 3,30 | 3,31 | 3,20 |


**Modo bloco_1MiB** (razao = original / comprimido)

| classe | stored | zlib-6 | zstd-1 | zstd-3 | zstd-9 | zstd-19 | zstd-22 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|---|
| texto | 1,00 | 3,16 | 2,84 | 3,18 | 3,55 | 3,93 | 3,93 | 3,99 | 4,42 |
| codigo | 1,00 | 2,79 | 2,83 | 3,02 | 3,19 | 3,32 | 3,32 | 3,37 | 3,19 |
| binario | 1,00 | 1,83 | 1,71 | 1,85 | 1,93 | 2,04 | 2,04 | 2,13 | 1,90 |
| cientifico | 1,00 | 2,13 | 1,92 | 2,11 | 2,26 | 2,40 | 2,40 | 2,74 | 2,82 |
| banco_de_dados | 1,00 | 2,71 | 2,68 | 2,85 | 2,96 | 3,05 | 3,05 | 3,34 | 3,49 |
| xml | 1,00 | 7,71 | 7,62 | 8,19 | 9,89 | 11,02 | 11,05 | 11,52 | 12,18 |
| dna | 1,00 | 3,45 | 3,29 | 3,31 | 3,47 | 4,00 | 4,00 | 3,80 | 3,70 |
| sint_logs | 1,00 | 4,07 | 4,09 | 4,05 | 4,59 | 5,37 | 5,37 | 5,42 | 6,13 |
| sint_incompressivel | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 |
| sint_ja_comprimido | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 |
| mix_real | 1,00 | 2,54 | 2,35 | 2,57 | 2,75 | 2,95 | 2,95 | 3,16 | 3,17 |


**Modo bloco_64KiB** (razao = original / comprimido)

| classe | stored | zlib-6 | zstd-1 | zstd-3 | zstd-9 | zstd-19 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|
| texto | 1,00 | 3,00 | 2,69 | 2,84 | 3,06 | 3,21 | 3,25 | 3,60 |
| codigo | 1,00 | 2,67 | 2,55 | 2,63 | 2,74 | 2,81 | 2,85 | 2,80 |
| binario | 1,00 | 1,81 | 1,70 | 1,75 | 1,82 | 1,90 | 1,97 | 1,82 |
| cientifico | 1,00 | 2,10 | 1,90 | 2,00 | 2,11 | 2,26 | 2,60 | 2,62 |
| banco_de_dados | 1,00 | 2,38 | 2,31 | 2,41 | 2,47 | 2,49 | 2,64 | 2,44 |
| xml | 1,00 | 7,11 | 6,27 | 6,64 | 7,63 | 8,03 | 8,24 | 8,80 |
| dna | 1,00 | 3,40 | 3,25 | 3,14 | 3,40 | 3,96 | 3,61 | 3,67 |
| sint_logs | 1,00 | 3,88 | 3,94 | 3,90 | 4,24 | 4,57 | 4,59 | 4,98 |
| sint_incompressivel | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 0,99 |
| sint_ja_comprimido | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 | 0,99 |
| mix_real | 1,00 | 2,45 | 2,25 | 2,36 | 2,49 | 2,62 | 2,79 | 2,80 |


## 5. Vazao por nucleo (MB/s, mediana) compressao / descompressao


**Modo inteiro**

| classe | stored | zlib-1 | zlib-6 | zlib-9 | zstd-1 | zstd-3 | zstd-3-long27 | zstd-9 | zstd-19 | zstd-22 | xz-1 | xz-6 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| texto | 12.829 / 12.249 | 147 / 387 | 34 / 421 | 17 / 423 | 461 / 1.640 | 323 / 1.325 | 268 / 1.505 | 74 / 1.585 | 4 / 1.234 | 3 / 1.207 | 27 / 164 | 3 / 221 | 3 / 204 | 22 / 53 |
| codigo | 8.947 / 12.161 | 144 / 437 | 59 / 472 | 41 / 475 | 719 / 2.131 | 529 / 2.101 | 361 / 2.116 | 135 / 2.167 | 6 / 1.293 | 4 / 1.333 | 31 / 159 | 6 / 180 | 5 / 167 | 23 / 56 |
| binario | 14.055 / 14.495 | 97 / 310 | 43 / 331 | 16 / 333 | 706 / 1.577 | 396 / 1.434 | 313 / 1.415 | 109 / 1.469 | 7 / 1.038 | 6 / 887 | 18 / 96 | 5 / 98 | 5 / 94 | 23 / 40 |
| cientifico | 12.661 / 9.991 | 117 / 366 | 40 / 379 | 20 / 382 | 747 / 1.510 | 339 / 1.304 | 263 / 1.222 | 94 / 1.219 | 4 / 966 | 4 / 962 | 20 / 107 | 5 / 108 | 3 / 104 | 22 / 50 |
| banco_de_dados | 12.882 / 9.443 | 141 / 418 | 59 / 444 | 39 / 453 | 652 / 1.717 | 432 / 1.731 | 330 / 1.760 | 99 / 1.670 | 5 / 1.228 | 4 / 1.166 | 26 / 133 | 4 / 151 | 4 / 142 | 24 / 49 |
| xml | 14.329 / 13.678 | 278 / 674 | 93 / 815 | 46 / 856 | 1.010 / 2.193 | 803 / 2.204 | 411 / 2.637 | 182 / 2.596 | 5 / 2.863 | 3 / 2.075 | 69 / 397 | 9 / 472 | 5 / 434 | 18 / 92 |
| dna | 17.316 / 13.942 | 145 / 380 | 15 / 499 | 2 / 540 | 404 / 1.438 | 386 / 1.468 | 308 / 1.536 | 59 / 1.495 | 3 / 2.019 | 3 / 1.702 | 24 / 118 | 2 / 208 | 2 / 201 | 21 / 46 |
| sint_logs | 10.560 / 12.407 | 196 / 485 | 63 / 493 | 30 / 506 | 583 / 1.435 | 424 / 1.531 | 327 / 1.593 | 93 / 1.666 | 3 / 1.444 | 3 / 1.436 | 32 / 197 | 3 / 200 | 3 / 185 | 24 / 62 |
| sint_incompressivel | 11.948 / 13.785 | 78 / 2.937 | 73 / 3.043 | 73 / 3.214 | 4.441 / 5.265 | 4.675 / 6.601 | 1.245 / 5.417 | 2.098 / 3.933 | 12 / 2.166 | 14 / 2.238 | 9 / 5.599 | 5 / 2.560 | 6 / 2.233 | 17 / 28 |
| sint_ja_comprimido | 11.297 / 13.755 | 78 / 2.926 | 73 / 3.201 | 73 / 2.982 | 5.220 / 8.024 | 4.549 / 7.211 | 1.257 / 6.041 | 2.296 / 4.403 | 12 / 2.197 | 14 / 2.126 | 9 / 6.849 | 5 / 4.639 | 6 / 2.275 | 17 / 28 |
| mix_real | 12.706 / 11.526 | 130 / 381 | 38 / 410 | 15 / 414 | 609 / 1.634 | 372 / 1.439 | 289 / 1.471 | 91 / 1.493 | 4 / 1.161 | 4 / 1.106 | 24 / 129 | 4 / 144 | 3 / 137 | 22 / 51 |


**Modo bloco_1MiB**

| classe | stored | zlib-6 | zstd-1 | zstd-3 | zstd-9 | zstd-19 | zstd-22 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|---|
| texto | 14.400 / 11.546 | 34 / 417 | 451 / 1.711 | 325 / 1.632 | 79 / 1.777 | 6 / 1.740 | 6 / 1.695 | 5 / 193 | 22 / 54 |
| codigo | 12.945 / 9.383 | 59 / 467 | 702 / 2.286 | 517 / 2.270 | 130 / 2.410 | 8 / 2.211 | 6 / 2.140 | 6 / 162 | 23 / 58 |
| binario | 14.129 / 13.595 | 43 / 331 | 679 / 1.628 | 411 / 1.524 | 120 / 1.633 | 10 / 1.250 | 8 / 1.226 | 7 / 96 | 23 / 40 |
| cientifico | 12.936 / 11.943 | 41 / 376 | 737 / 1.582 | 348 / 1.412 | 105 / 1.523 | 8 / 1.336 | 6 / 1.296 | 5 / 105 | 22 / 51 |
| banco_de_dados | 13.635 / 10.509 | 60 / 437 | 629 / 1.747 | 453 / 1.817 | 108 / 1.917 | 8 / 1.811 | 8 / 1.749 | 6 / 141 | 24 / 49 |
| xml | 15.154 / 15.220 | 94 / 823 | 990 / 2.388 | 805 / 2.402 | 177 / 3.009 | 5 / 2.856 | 3 / 2.733 | 6 / 442 | 19 / 92 |
| dna | 15.899 / 13.020 | 15 / 504 | 402 / 1.486 | 375 / 1.576 | 58 / 1.520 | 4 / 2.127 | 4 / 2.093 | 4 / 182 | 22 / 48 |
| sint_logs | 14.155 / 11.573 | 63 / 486 | 566 / 1.529 | 446 / 1.764 | 99 / 1.865 | 6 / 2.562 | 6 / 2.395 | 5 / 185 | 24 / 61 |
| sint_incompressivel | 14.955 / 11.293 | 74 / 3.268 | 5.357 / 6.638 | 5.539 / 8.369 | 1.510 / 6.876 | 42 / 8.789 | 39 / 7.317 | 11 / 6.854 | 17 / 28 |
| sint_ja_comprimido | 11.391 / 13.605 | 74 / 3.251 | 6.083 / 8.510 | 5.126 / 7.929 | 1.920 / 7.513 | 43 / 8.985 | 41 / 7.548 | 11 / 6.833 | 17 / 28 |
| mix_real | 13.781 / 11.811 | 39 / 407 | 596 / 1.707 | 378 / 1.611 | 98 / 1.736 | 7 / 1.581 | 6 / 1.538 | 5 / 135 | 22 / 51 |


## 6. Penalidade de bloco (aumento do tamanho comprimido vs arquivo inteiro, %)


**bloco_1MiB**

| classe | zlib-6 | zstd-1 | zstd-3 | zstd-9 | zstd-19 | zstd-22 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|---|
| texto | 0,3 | 0,5 | 1,8 | 4,4 | 9,7 | 9,7 | 9,5 | 2,0 |
| codigo | 0,3 | 0,7 | 5,4 | 9,3 | 11,9 | 12,0 | 11,8 | 1,4 |
| binario | 0,1 | 0,3 | 0,8 | 2,3 | 2,2 | 2,2 | 2,2 | -0,2 |
| cientifico | 0,1 | 0,1 | 0,6 | 3,2 | 2,9 | 2,9 | 2,0 | 0,5 |
| banco_de_dados | 0,8 | 1,0 | 1,1 | 1,9 | 6,1 | 6,1 | 5,8 | 3,0 |
| xml | 0,7 | 1,1 | 2,7 | 4,6 | 7,1 | 7,2 | 6,7 | -0,5 |
| dna | 0,1 | -0,0 | 0,6 | 0,2 | 0,8 | 0,8 | 3,0 | 0,1 |
| sint_logs | 0,3 | 0,2 | 0,7 | -0,0 | 6,6 | 6,6 | 6,3 | 1,9 |
| sint_incompressivel | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 |
| sint_ja_comprimido | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 |
| mix_real | 0,2 | 0,3 | 1,3 | 3,5 | 5,0 | 5,0 | 4,8 | 0,9 |


**bloco_64KiB**

| classe | zlib-6 | zstd-1 | zstd-3 | zstd-9 | zstd-19 | xz-9e | bzip2-9 |
|---|---|---|---|---|---|---|---|
| texto | 5,7 | 6,2 | 14,0 | 21,0 | 34,3 | 34,4 | 25,5 |
| codigo | 4,7 | 11,8 | 20,9 | 27,1 | 32,3 | 32,3 | 15,8 |
| binario | 1,1 | 1,4 | 6,8 | 8,5 | 9,9 | 10,2 | 4,0 |
| cientifico | 1,6 | 1,0 | 6,1 | 10,6 | 9,1 | 7,6 | 8,0 |
| banco_de_dados | 14,6 | 17,0 | 19,5 | 21,9 | 30,2 | 33,9 | 47,5 |
| xml | 9,3 | 23,0 | 26,7 | 35,5 | 46,9 | 49,1 | 37,7 |
| dna | 1,5 | 1,1 | 6,1 | 2,2 | 1,6 | 8,3 | 0,9 |
| sint_logs | 5,3 | 3,9 | 4,5 | 8,0 | 25,2 | 25,5 | 25,4 |
| sint_incompressivel | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,1 | 0,5 |
| sint_ja_comprimido | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,1 | 0,5 |
| mix_real | 3,8 | 4,5 | 10,3 | 14,5 | 17,9 | 18,4 | 14,2 |


## 7. Dicionario treinado (zstd, blocos de 64 KiB, dicionario ~110 KB, treino disjunto do teste)

| classe | config | arquivos | razao sem | razao com | ganho de razao % | reducao de tamanho % | comp MB/s sem->com | decomp MB/s sem->com | bit-exato |
|---|---|---|---|---|---|---|---|---|---|
| codigo | zstd-3 | samba | 4,57 | 4,67 | 2,1 | 2,1 | 568 -> 373 | 1.427 -> 1.217 | sim |
| codigo | zstd-9 | samba | 5,12 | 5,33 | 4,0 | 3,9 | 118 -> 90 | 1.531 -> 1.247 | sim |
| sint_logs | zstd-3 | logs.log | 3,90 | 3,76 | -3,7 | -3,8 | 501 -> 285 | 1.613 -> 1.003 | sim |
| sint_logs | zstd-9 | logs.log | 4,24 | 4,29 | 1,2 | 1,1 | 88 -> 60 | 1.388 -> 1.039 | sim |
| texto | zstd-3 | bible.txt,dickens,reymont,webster,world192.txt | 2,85 | 3,06 | 7,7 | 7,1 | 310 -> 236 | 1.272 -> 862 | sim |
| texto | zstd-9 | bible.txt,dickens,reymont,webster,world192.txt | 3,07 | 3,40 | 10,5 | 9,5 | 69 -> 44 | 1.380 -> 916 | sim |
| xml | zstd-3 | xml | 6,97 | 6,94 | -0,4 | -0,4 | 717 -> 475 | 1.757 -> 1.852 | sim |
| xml | zstd-9 | xml | 8,05 | 8,10 | 0,7 | 0,7 | 148 -> 128 | 2.567 -> 1.974 | sim |


Delta de valor liquido por GB (dicionario - sem dicionario), US$/GB, cenario nuvem+s3_standard, camadas quente/morno/frio (nao inclui custo de treino, versionamento e armazenamento de dicionarios):

| classe | config | quente | morno | frio |
|---|---|---|---|---|
| codigo | zstd-3 | 0,0061 | 0,0062 | 0,0063 |
| codigo | zstd-9 | 0,0096 | 0,0103 | 0,0104 |
| sint_logs | zstd-3 | -0,0141 | -0,0136 | -0,0136 |
| sint_logs | zstd-9 | 0,0024 | 0,0035 | 0,0036 |
| texto | zstd-3 | 0,0339 | 0,0344 | 0,0345 |
| texto | zstd-9 | 0,0406 | 0,0423 | 0,0424 |
| xml | zstd-3 | -0,0008 | -0,0008 | -0,0008 |
| xml | zstd-9 | -0,0001 | 0,0011 | 0,0011 |


## 8. Requisito 5 PB/mes: nucleos e custo (modo bloco_1MiB)

N medio e N pico incluem a eficiencia de escala medida (zstd-3, interpolada, constante acima de N=32). Custo mensal = N medio x preco x 730 h. `cabe64` = N pico <= 64.


**texto**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 14.400 | 11.546 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 2,84 | 64,8 | 451 | 1.711 | 4,4 | 40,6 | sim | 142,07 | 43,00 | * |
| zlib-6 | 3,16 | 68,4 | 34 | 417 | 135,3 | 541,1 | nao | 4.404 | 1.333 |  |
| zstd-3 | 3,18 | 68,6 | 325 | 1.632 | 6,2 | 56,3 | sim | 200,45 | 60,67 | * |
| zstd-9 | 3,55 | 71,9 | 79 | 1.777 | 57,6 | 230,3 | nao | 1.874 | 567,34 | * |
| zstd-19 | 3,93 | 74,6 | 6 | 1.740 | 791,0 | 3.164 | nao | 25.755 | 7.796 |  |
| zstd-22 | 3,93 | 74,6 | 6 | 1.695 | 813,4 | 3.254 | nao | 26.483 | 8.016 |  |
| xz-9e | 3,99 | 75,0 | 5 | 193 | 927,0 | 3.708 | nao | 30.180 | 9.135 |  |
| bzip2-9 | 4,42 | 77,4 | 22 | 54 | 206,3 | 825,0 | nao | 6.715 | 2.033 | * |


**codigo**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 12.945 | 9.383 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zlib-6 | 2,79 | 64,2 | 59 | 467 | 77,5 | 309,9 | nao | 2.522 | 763,45 |  |
| zstd-1 | 2,83 | 64,6 | 702 | 2.286 | 2,8 | 18,0 | sim | 90,08 | 27,27 | * |
| zstd-3 | 3,02 | 66,8 | 517 | 2.270 | 3,8 | 35,4 | sim | 123,23 | 37,30 | * |
| zstd-9 | 3,19 | 68,6 | 130 | 2.410 | 35,2 | 140,7 | nao | 1.145 | 346,55 | * |
| bzip2-9 | 3,19 | 68,7 | 23 | 58 | 198,0 | 792,2 | nao | 6.448 | 1.952 | * |
| zstd-19 | 3,32 | 69,9 | 8 | 2.211 | 552,1 | 2.208 | nao | 17.974 | 5.441 | * |
| zstd-22 | 3,32 | 69,9 | 6 | 2.140 | 828,9 | 3.316 | nao | 26.989 | 8.169 |  |
| xz-9e | 3,37 | 70,3 | 6 | 162 | 726,5 | 2.906 | nao | 23.654 | 7.160 | * |


**binario**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 14.129 | 13.595 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 1,71 | 41,7 | 679 | 1.628 | 2,9 | 19,1 | sim | 93,09 | 28,18 | * |
| zlib-6 | 1,83 | 45,2 | 43 | 331 | 106,2 | 424,9 | nao | 3.458 | 1.047 |  |
| zstd-3 | 1,85 | 46,0 | 411 | 1.524 | 4,8 | 44,5 | sim | 156,45 | 47,36 | * |
| bzip2-9 | 1,90 | 47,4 | 23 | 40 | 202,3 | 809,4 | nao | 6.588 | 1.994 |  |
| zstd-9 | 1,93 | 48,1 | 120 | 1.633 | 38,1 | 152,6 | nao | 1.242 | 375,90 | * |
| zstd-19 | 2,04 | 51,0 | 10 | 1.250 | 444,3 | 1.777 | nao | 14.464 | 4.378 | * |
| zstd-22 | 2,04 | 51,0 | 8 | 1.226 | 551,6 | 2.206 | nao | 17.959 | 5.436 | * |
| xz-9e | 2,13 | 53,0 | 7 | 96 | 645,3 | 2.581 | nao | 21.008 | 6.359 | * |


**cientifico**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 12.936 | 11.943 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 1,92 | 47,9 | 737 | 1.582 | 2,6 | 16,6 | sim | 85,67 | 25,93 | * |
| zstd-3 | 2,11 | 52,7 | 348 | 1.412 | 5,7 | 52,6 | sim | 186,31 | 56,39 | * |
| zlib-6 | 2,13 | 53,1 | 41 | 376 | 112,7 | 450,9 | nao | 3.670 | 1.111 |  |
| zstd-9 | 2,26 | 55,8 | 105 | 1.523 | 43,7 | 174,7 | nao | 1.422 | 430,37 | * |
| zstd-19 | 2,40 | 58,3 | 8 | 1.336 | 599,8 | 2.399 | nao | 19.529 | 5.911 |  |
| zstd-22 | 2,40 | 58,3 | 6 | 1.296 | 775,9 | 3.104 | nao | 25.263 | 7.647 |  |
| xz-9e | 2,74 | 63,6 | 5 | 105 | 926,0 | 3.704 | nao | 30.148 | 9.126 |  |
| bzip2-9 | 2,82 | 64,5 | 22 | 51 | 207,4 | 829,4 | nao | 6.751 | 2.044 | * |


**banco_de_dados**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 13.635 | 10.509 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 2,68 | 62,6 | 629 | 1.747 | 3,1 | 22,5 | sim | 100,76 | 30,50 | * |
| zlib-6 | 2,71 | 63,1 | 60 | 437 | 76,7 | 306,7 | nao | 2.496 | 755,55 |  |
| zstd-3 | 2,85 | 64,9 | 453 | 1.817 | 4,3 | 40,4 | sim | 141,45 | 42,81 | * |
| zstd-9 | 2,96 | 66,2 | 108 | 1.917 | 42,6 | 170,2 | nao | 1.385 | 419,35 | * |
| zstd-22 | 3,05 | 67,2 | 8 | 1.749 | 590,5 | 2.362 | nao | 19.225 | 5.819 |  |
| zstd-19 | 3,05 | 67,2 | 8 | 1.811 | 573,1 | 2.292 | nao | 18.658 | 5.648 |  |
| xz-9e | 3,34 | 70,1 | 6 | 141 | 704,9 | 2.820 | nao | 22.952 | 6.947 |  |
| bzip2-9 | 3,49 | 71,3 | 24 | 49 | 186,9 | 747,8 | nao | 6.086 | 1.842 | * |


**xml**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 15.154 | 15.220 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 7,62 | 86,9 | 990 | 2.388 | 1,9 | 8,3 | sim | 63,44 | 19,20 | * |
| zlib-6 | 7,71 | 87,0 | 94 | 823 | 48,5 | 194,0 | nao | 1.579 | 477,97 |  |
| zstd-3 | 8,19 | 87,8 | 805 | 2.402 | 2,4 | 12,2 | sim | 78,28 | 23,69 | * |
| zstd-9 | 9,89 | 89,9 | 177 | 3.009 | 17,7 | 103,3 | nao | 575,74 | 174,27 | * |
| zstd-19 | 11,02 | 90,9 | 5 | 2.856 | 858,5 | 3.434 | nao | 27.950 | 8.460 |  |
| zstd-22 | 11,05 | 91,0 | 3 | 2.733 | 1.466 | 5.863 | nao | 47.720 | 14.444 |  |
| xz-9e | 11,52 | 91,3 | 6 | 442 | 800,2 | 3.201 | nao | 26.052 | 7.886 |  |
| bzip2-9 | 12,18 | 91,8 | 19 | 92 | 244,1 | 976,4 | nao | 7.947 | 2.406 | * |


**dna**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 15.899 | 13.020 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 3,29 | 69,6 | 402 | 1.486 | 4,9 | 45,5 | sim | 160,23 | 48,50 | * |
| zstd-3 | 3,31 | 69,8 | 375 | 1.576 | 5,3 | 48,8 | sim | 172,41 | 52,19 | * |
| zlib-6 | 3,45 | 71,0 | 15 | 504 | 311,0 | 1.244 | nao | 10.126 | 3.065 |  |
| zstd-9 | 3,47 | 71,2 | 58 | 1.520 | 78,9 | 315,7 | nao | 2.570 | 777,86 | * |
| bzip2-9 | 3,70 | 73,0 | 22 | 48 | 211,4 | 845,5 | nao | 6.882 | 2.083 | * |
| xz-9e | 3,80 | 73,7 | 4 | 182 | 1.275 | 5.098 | nao | 41.496 | 12.560 |  |
| zstd-19 | 4,00 | 75,0 | 4 | 2.127 | 1.075 | 4.300 | nao | 35.003 | 10.595 | * |
| zstd-22 | 4,00 | 75,0 | 4 | 2.093 | 1.079 | 4.317 | nao | 35.141 | 10.637 | * |


**sint_logs**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 14.155 | 11.573 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-3 | 4,05 | 75,3 | 446 | 1.764 | 4,4 | 41,1 | sim | 143,85 | 43,54 |  |
| zlib-6 | 4,07 | 75,5 | 63 | 486 | 72,4 | 289,6 | nao | 2.357 | 713,45 |  |
| zstd-1 | 4,09 | 75,5 | 566 | 1.529 | 3,4 | 32,3 | sim | 112,29 | 33,99 | * |
| zstd-9 | 4,59 | 78,2 | 99 | 1.865 | 46,4 | 185,5 | nao | 1.510 | 457,14 | * |
| zstd-22 | 5,37 | 81,4 | 6 | 2.395 | 789,9 | 3.160 | nao | 25.717 | 7.784 |  |
| zstd-19 | 5,37 | 81,4 | 6 | 2.562 | 747,9 | 2.991 | nao | 24.349 | 7.370 |  |
| xz-9e | 5,42 | 81,5 | 5 | 185 | 873,3 | 3.493 | nao | 28.434 | 8.607 |  |
| bzip2-9 | 6,13 | 83,7 | 24 | 61 | 191,5 | 765,9 | nao | 6.234 | 1.887 | * |


**sint_incompressivel**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| bzip2-9 | 1,00 | -0,5 | 17 | 28 | 261,5 | 1.046 | nao | 8.514 | 2.577 |  |
| zlib-6 | 1,00 | -0,0 | 74 | 3.268 | 62,1 | 248,4 | nao | 2.021 | 611,88 |  |
| xz-9e | 1,00 | -0,0 | 11 | 6.854 | 420,6 | 1.682 | nao | 13.694 | 4.145 |  |
| zstd-1 | 1,00 | -0,0 | 5.357 | 6.638 | 0,4 | 1,4 | sim | 11,72 | 3,55 |  |
| zstd-22 | 1,00 | -0,0 | 39 | 7.317 | 118,2 | 472,6 | nao | 3.847 | 1.164 |  |
| zstd-19 | 1,00 | -0,0 | 42 | 8.789 | 109,0 | 435,8 | nao | 3.547 | 1.074 |  |
| zstd-9 | 1,00 | -0,0 | 1.510 | 6.876 | 1,3 | 5,3 | sim | 41,61 | 12,59 |  |
| zstd-3 | 1,00 | -0,0 | 5.539 | 8.369 | 0,3 | 1,4 | sim | 11,34 | 3,43 |  |
| stored | 1,00 | 0,0 | 14.955 | 11.293 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |


**sint_ja_comprimido**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| bzip2-9 | 1,00 | -0,5 | 17 | 28 | 261,8 | 1.047 | nao | 8.523 | 2.580 |  |
| zlib-6 | 1,00 | -0,0 | 74 | 3.251 | 61,9 | 247,6 | nao | 2.015 | 610,06 |  |
| xz-9e | 1,00 | -0,0 | 11 | 6.833 | 421,9 | 1.688 | nao | 13.736 | 4.158 |  |
| zstd-1 | 1,00 | -0,0 | 6.083 | 8.510 | 0,3 | 1,3 | sim | 10,32 | 3,13 |  |
| zstd-22 | 1,00 | -0,0 | 41 | 7.548 | 111,9 | 447,8 | nao | 3.645 | 1.103 |  |
| zstd-19 | 1,00 | -0,0 | 43 | 8.985 | 106,3 | 425,3 | nao | 3.462 | 1.048 |  |
| zstd-9 | 1,00 | -0,0 | 1.920 | 7.513 | 1,0 | 4,1 | sim | 32,72 | 9,90 |  |
| zstd-3 | 1,00 | -0,0 | 5.126 | 7.929 | 0,4 | 1,5 | sim | 12,25 | 3,71 |  |
| stored | 1,00 | 0,0 | 11.391 | 13.605 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |


**mix_real**

| config | razao x | economia % | comp MB/s | decomp MB/s | N medio | N pico | cabe64 | custo/mes nuvem US$ | custo/mes on-prem US$ | Pareto |
|---|---|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 0,0 | 13.781 | 11.811 | 0,0 | 0,0 | sim | 0,0000 | 0,0000 | * |
| zstd-1 | 2,35 | 57,4 | 596 | 1.707 | 3,3 | 26,1 | sim | 106,54 | 32,25 | * |
| zlib-6 | 2,54 | 60,7 | 39 | 407 | 118,1 | 472,4 | nao | 3.845 | 1.164 |  |
| zstd-3 | 2,57 | 61,1 | 378 | 1.611 | 5,3 | 48,4 | sim | 170,95 | 51,75 | * |
| zstd-9 | 2,75 | 63,7 | 98 | 1.736 | 46,8 | 187,1 | nao | 1.523 | 461,08 | * |
| zstd-19 | 2,95 | 66,1 | 7 | 1.581 | 661,8 | 2.647 | nao | 21.546 | 6.522 |  |
| zstd-22 | 2,95 | 66,1 | 6 | 1.538 | 793,7 | 3.175 | nao | 25.842 | 7.822 |  |
| xz-9e | 3,16 | 68,3 | 5 | 135 | 862,0 | 3.448 | nao | 28.064 | 8.495 |  |
| bzip2-9 | 3,17 | 68,5 | 22 | 51 | 206,0 | 824,0 | nao | 6.707 | 2.030 | * |


Sensibilidade do N pico (classe mix_real, modo bloco_1MiB) a curva de eficiencia de escala e ao fator de vazao do nucleo de servidor:

| config | P-cores fisicos (N<=8), fator 1,0 | N<=16, fator 1,0 | N<=32 (base), fator 1,0 | N<=32, fator 0,6 |
|---|---|---|---|---|
| zstd-1 | 13,7 (ef 0,95) | 20,2 (ef 0,64) | 26,1 (ef 0,50) | 51,2 (ef 0,42) |
| zstd-3 | 21,6 (ef 0,95) | 31,9 (ef 0,64) | 48,4 (ef 0,42) | 80,7 (ef 0,42) *>64 |
| zstd-9 | 83,3 (ef 0,95) *>64 | 123,2 (ef 0,64) *>64 | 187,1 (ef 0,42) *>64 | 311,9 (ef 0,42) *>64 |
| zstd-19 | 1.178 (ef 0,95) *>64 | 1.743 (ef 0,64) *>64 | 2.647 (ef 0,42) *>64 | 4.412 (ef 0,42) *>64 |
| xz-9e | 1.534 (ef 0,95) *>64 | 2.270 (ef 0,64) *>64 | 3.448 (ef 0,42) *>64 | 5.746 (ef 0,42) *>64 |


Configuracoes que cumprem 5 PB/mes com <= 64 nucleos no pico (modo bloco_1MiB):

| classe | configuracoes (N pico <= 64) |
|---|---|
| texto | stored, zstd-1, zstd-3 |
| codigo | stored, zstd-1, zstd-3 |
| binario | stored, zstd-1, zstd-3 |
| cientifico | stored, zstd-1, zstd-3 |
| banco_de_dados | stored, zstd-1, zstd-3 |
| xml | stored, zstd-1, zstd-3 |
| dna | stored, zstd-1, zstd-3 |
| sint_logs | stored, zstd-3, zstd-1 |
| sint_incompressivel | zstd-1, zstd-9, zstd-3, stored |
| sint_ja_comprimido | zstd-1, zstd-9, zstd-3, stored |
| mix_real | stored, zstd-1, zstd-3 |


## 9. Configuracao otima por classe, camada e cenario (maior valor liquido; modo bloco_1MiB)

Valor liquido por GB = economia de armazenamento (T=60 meses) - custo de compressao - n x custo de descompressao. Tabela com cenario nuvem+s3_standard; demais cenarios na tabela seguinte.

| classe | camada | config otima | razao x | comp MB/s | decomp MB/s | N pico | custo CPU/mes nuvem US$ | liquido US$/GB | liquido US$/coorte mensal |
|---|---|---|---|---|---|---|---|---|---|
| texto | quente | zstd-19 | 3,93 | 6 | 1.740 | 3.164 | 25.755 | 1,02 | 5.106.131 |
| texto | morno | bzip2-9 | 4,42 | 22 | 54 | 825,0 | 6.715 | 1,06 | 5.309.441 |
| texto | frio | bzip2-9 | 4,42 | 22 | 54 | 825,0 | 6.715 | 1,07 | 5.332.079 |
| codigo | quente | zstd-19 | 3,32 | 8 | 2.211 | 2.208 | 17.974 | 0,9584 | 4.792.238 |
| codigo | morno | xz-9e | 3,37 | 6 | 162 | 2.906 | 23.654 | 0,9644 | 4.821.953 |
| codigo | frio | xz-9e | 3,37 | 6 | 162 | 2.906 | 23.654 | 0,9659 | 4.829.564 |
| binario | quente | zstd-19 | 2,04 | 10 | 1.250 | 1.777 | 14.464 | 0,6963 | 3.481.271 |
| binario | morno | xz-9e | 2,13 | 7 | 96 | 2.581 | 21.008 | 0,7250 | 3.625.103 |
| binario | frio | xz-9e | 2,13 | 7 | 96 | 2.581 | 21.008 | 0,7276 | 3.637.940 |
| cientifico | quente | xz-9e | 2,74 | 5 | 105 | 3.704 | 30.148 | 0,8232 | 4.116.175 |
| cientifico | morno | bzip2-9 | 2,82 | 22 | 51 | 829,4 | 6.751 | 0,8837 | 4.418.459 |
| cientifico | frio | bzip2-9 | 2,82 | 22 | 51 | 829,4 | 6.751 | 0,8885 | 4.442.580 |
| banco_de_dados | quente | xz-9e | 3,34 | 6 | 141 | 2.820 | 22.952 | 0,9267 | 4.633.560 |
| banco_de_dados | morno | bzip2-9 | 3,49 | 24 | 49 | 747,8 | 6.086 | 0,9782 | 4.890.953 |
| banco_de_dados | frio | bzip2-9 | 3,49 | 24 | 49 | 747,8 | 6.086 | 0,9832 | 4.915.866 |
| xml | quente | zstd-19 | 11,02 | 5 | 2.856 | 3.434 | 27.950 | 1,25 | 6.237.386 |
| xml | morno | bzip2-9 | 12,18 | 19 | 92 | 976,4 | 7.947 | 1,26 | 6.311.902 |
| xml | frio | bzip2-9 | 12,18 | 19 | 92 | 976,4 | 7.947 | 1,27 | 6.325.303 |
| dna | quente | zstd-19 | 4,00 | 4 | 2.127 | 4.300 | 35.003 | 1,03 | 5.127.753 |
| dna | morno | zstd-19 | 4,00 | 4 | 2.127 | 4.300 | 35.003 | 1,03 | 5.138.966 |
| dna | frio | zstd-19 | 4,00 | 4 | 2.127 | 4.300 | 35.003 | 1,03 | 5.139.544 |
| sint_logs | quente | zstd-19 | 5,37 | 6 | 2.562 | 2.991 | 24.349 | 1,12 | 5.580.235 |
| sint_logs | morno | bzip2-9 | 6,13 | 24 | 61 | 765,9 | 6.234 | 1,15 | 5.747.089 |
| sint_logs | frio | bzip2-9 | 6,13 | 24 | 61 | 765,9 | 6.234 | 1,15 | 5.767.124 |
| sint_incompressivel | quente | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_incompressivel | morno | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_incompressivel | frio | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | quente | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | morno | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | frio | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| mix_real | quente | zstd-19 | 2,95 | 7 | 1.581 | 2.647 | 21.546 | 0,9042 | 4.521.063 |
| mix_real | morno | bzip2-9 | 3,17 | 22 | 51 | 824,0 | 6.707 | 0,9391 | 4.695.335 |
| mix_real | frio | bzip2-9 | 3,17 | 22 | 51 | 824,0 | 6.707 | 0,9439 | 4.719.258 |


Otimo por cenario (quente / morno / frio):

| classe | nuvem+s3_standard | nuvem+glacier_ir | onprem+onprem |
|---|---|---|---|
| texto | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 |
| codigo | zstd-19 / xz-9e / xz-9e | zstd-9 / zstd-9 / zstd-9 | zstd-19 / zstd-19 / xz-9e |
| binario | zstd-19 / xz-9e / xz-9e | zstd-19 / xz-9e / xz-9e | zstd-19 / xz-9e / xz-9e |
| cientifico | xz-9e / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 |
| banco_de_dados | xz-9e / bzip2-9 / bzip2-9 | zstd-9 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 |
| xml | zstd-19 / bzip2-9 / bzip2-9 | zstd-9 / bzip2-9 / bzip2-9 | zstd-9 / bzip2-9 / bzip2-9 |
| dna | zstd-19 / zstd-19 / zstd-19 | zstd-19 / zstd-19 / bzip2-9 | zstd-19 / zstd-19 / zstd-19 |
| sint_logs | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 |
| sint_incompressivel | stored / stored / stored | stored / stored / stored | stored / stored / stored |
| sint_ja_comprimido | stored / stored / stored | stored / stored / stored | stored / stored / stored |
| mix_real | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 | zstd-19 / bzip2-9 / bzip2-9 |


Otimo com restricao do requisito (N pico <= 64 nucleos), cenario nuvem+s3_standard, modo bloco_1MiB:

| classe | camada | config otima (<=64) | razao x | comp MB/s | decomp MB/s | N pico | custo CPU/mes nuvem US$ | liquido US$/GB | perda vs irrestrito US$/GB |
|---|---|---|---|---|---|---|---|---|---|
| texto | quente | zstd-3 | 3,18 | 325 | 1.632 | 56,3 | 200,45 | 0,9457 | 0,0755 |
| texto | morno | zstd-3 | 3,18 | 325 | 1.632 | 56,3 | 200,45 | 0,9465 | 0,1154 |
| texto | frio | zstd-3 | 3,18 | 325 | 1.632 | 56,3 | 200,45 | 0,9466 | 0,1199 |
| codigo | quente | zstd-3 | 3,02 | 517 | 2.270 | 35,4 | 123,23 | 0,9217 | 0,0367 |
| codigo | morno | zstd-3 | 3,02 | 517 | 2.270 | 35,4 | 123,23 | 0,9222 | 0,0421 |
| codigo | frio | zstd-3 | 3,02 | 517 | 2.270 | 35,4 | 123,23 | 0,9223 | 0,0436 |
| binario | quente | zstd-3 | 1,85 | 411 | 1.524 | 44,5 | 156,45 | 0,6346 | 0,0617 |
| binario | morno | zstd-3 | 1,85 | 411 | 1.524 | 44,5 | 156,45 | 0,6354 | 0,0896 |
| binario | frio | zstd-3 | 1,85 | 411 | 1.524 | 44,5 | 156,45 | 0,6354 | 0,0922 |
| cientifico | quente | zstd-3 | 2,11 | 348 | 1.412 | 52,6 | 186,31 | 0,7261 | 0,0972 |
| cientifico | morno | zstd-3 | 2,11 | 348 | 1.412 | 52,6 | 186,31 | 0,7270 | 0,1567 |
| cientifico | frio | zstd-3 | 2,11 | 348 | 1.412 | 52,6 | 186,31 | 0,7270 | 0,1615 |
| banco_de_dados | quente | zstd-3 | 2,85 | 453 | 1.817 | 40,4 | 141,45 | 0,8946 | 0,0321 |
| banco_de_dados | morno | zstd-3 | 2,85 | 453 | 1.817 | 40,4 | 141,45 | 0,8953 | 0,0829 |
| banco_de_dados | frio | zstd-3 | 2,85 | 453 | 1.817 | 40,4 | 141,45 | 0,8953 | 0,0878 |
| xml | quente | zstd-3 | 8,19 | 805 | 2.402 | 12,2 | 78,28 | 1,21 | 0,0365 |
| xml | morno | zstd-3 | 8,19 | 805 | 2.402 | 12,2 | 78,28 | 1,21 | 0,0509 |
| xml | frio | zstd-3 | 8,19 | 805 | 2.402 | 12,2 | 78,28 | 1,21 | 0,0535 |
| dna | quente | zstd-3 | 3,31 | 375 | 1.576 | 48,8 | 172,41 | 0,9623 | 0,0633 |
| dna | morno | zstd-3 | 3,31 | 375 | 1.576 | 48,8 | 172,41 | 0,9630 | 0,0647 |
| dna | frio | zstd-3 | 3,31 | 375 | 1.576 | 48,8 | 172,41 | 0,9631 | 0,0648 |
| sint_logs | quente | zstd-1 | 4,09 | 566 | 1.529 | 32,3 | 112,29 | 1,04 | 0,0745 |
| sint_logs | morno | zstd-1 | 4,09 | 566 | 1.529 | 32,3 | 112,29 | 1,04 | 0,1071 |
| sint_logs | frio | zstd-1 | 4,09 | 566 | 1.529 | 32,3 | 112,29 | 1,04 | 0,1111 |
| sint_incompressivel | quente | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_incompressivel | morno | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_incompressivel | frio | stored | 1,00 | 14.955 | 11.293 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | quente | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | morno | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| sint_ja_comprimido | frio | stored | 1,00 | 11.391 | 13.605 | 0,0 | 0,0000 | 0,0000 | 0,0000 |
| mix_real | quente | zstd-3 | 2,57 | 378 | 1.611 | 48,4 | 170,95 | 0,8417 | 0,0625 |
| mix_real | morno | zstd-3 | 2,57 | 378 | 1.611 | 48,4 | 170,95 | 0,8425 | 0,0966 |
| mix_real | frio | zstd-3 | 2,57 | 378 | 1.611 | 48,4 | 170,95 | 0,8425 | 0,1013 |


Otimo restrito por cenario (quente / morno / frio):

| classe | nuvem+s3_standard | nuvem+glacier_ir | onprem+onprem |
|---|---|---|---|
| texto | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| codigo | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| binario | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| cientifico | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| banco_de_dados | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| xml | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| dna | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |
| sint_logs | zstd-1 / zstd-1 / zstd-1 | zstd-1 / zstd-1 / zstd-1 | zstd-1 / zstd-1 / zstd-1 |
| sint_incompressivel | stored / stored / stored | stored / stored / stored | stored / stored / stored |
| sint_ja_comprimido | stored / stored / stored | stored / stored / stored | stored / stored / stored |
| mix_real | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 | zstd-3 / zstd-3 / zstd-3 |


Otimo no modo inteiro (arquivo inteiro; referencia superior, sem acesso aleatorio), cenario nuvem+s3_standard:

| classe | quente | morno | frio |
|---|---|---|---|
| texto | zstd-19 | bzip2-9 | bzip2-9 |
| codigo | zstd-19 | xz-6 | xz-6 |
| binario | zstd-19 | xz-6 | xz-6 |
| cientifico | xz-6 | bzip2-9 | bzip2-9 |
| banco_de_dados | xz-6 | bzip2-9 | bzip2-9 |
| xml | zstd-19 | bzip2-9 | bzip2-9 |
| dna | zstd-19 | zstd-19 | zstd-19 |
| sint_logs | zstd-19 | bzip2-9 | bzip2-9 |
| sint_incompressivel | stored | stored | stored |
| sint_ja_comprimido | stored | stored | stored |
| mix_real | zstd-19 | xz-6 | xz-6 |


## 10. Analise marginal (modo bloco_1MiB, cenario nuvem+s3_standard, camada morno)

Cadeia zstd-1 -> zstd-3 -> zstd-9 -> zstd-19 -> xz-9e (ordem de razao tipica; a ordenacao completa por razao de cada classe, todas as camadas e cenarios esta em `economia.json`, campo `marginal`). Ganho = economia de armazenamento adicional por GB; custo = CPU adicional por GB (compressao + n x descompressao); razao = ganho / custo.

| classe | passo | delta economia (pp) | ganho armazenamento US$/GB | custo CPU US$/GB | ganho/custo | delta liquido US$/GB | vale |
|---|---|---|---|---|---|---|---|
| texto | zstd-1 -> zstd-3 | 3,79 | 0,0522 | 0,0000 | 3.654 | 0,0522 | sim |
| texto | zstd-3 -> zstd-9 | 3,27 | 0,0451 | 0,0004 | 104,7 | 0,0447 | sim |
| texto | zstd-9 -> zstd-19 | 2,71 | 0,0375 | 0,0047 | 7,9 | 0,0327 | sim |
| texto | zstd-19 -> xz-9e | 0,37 | 0,0051 | 0,0020 | 2,5 | 0,0031 | sim |
| codigo | zstd-1 -> zstd-3 | 2,20 | 0,0303 | 0,0000 | 4.188 | 0,0303 | sim |
| codigo | zstd-3 -> zstd-9 | 1,80 | 0,0248 | 0,0003 | 89,5 | 0,0246 | sim |
| codigo | zstd-9 -> zstd-19 | 1,24 | 0,0171 | 0,0033 | 5,1 | 0,0138 | sim |
| codigo | zstd-19 -> xz-9e | 0,46 | 0,0063 | 0,0026 | 2,5 | 0,0038 | sim |
| binario | zstd-1 -> zstd-3 | 4,39 | 0,0605 | 0,0000 | 3.682 | 0,0605 | sim |
| binario | zstd-3 -> zstd-9 | 2,08 | 0,0287 | 0,0003 | 88,3 | 0,0284 | sim |
| binario | zstd-9 -> zstd-19 | 2,82 | 0,0389 | 0,0027 | 14,7 | 0,0363 | sim |
| binario | zstd-19 -> xz-9e | 2,08 | 0,0287 | 0,0037 | 7,7 | 0,0250 | sim |
| cientifico | zstd-1 -> zstd-3 | 4,82 | 0,0666 | 0,0000 | 2.486 | 0,0665 | sim |
| cientifico | zstd-3 -> zstd-9 | 3,11 | 0,0430 | 0,0004 | 118,7 | 0,0426 | sim |
| cientifico | zstd-9 -> zstd-19 | 2,51 | 0,0346 | 0,0036 | 9,6 | 0,0310 | sim |
| cientifico | zstd-19 -> xz-9e | 5,26 | 0,0726 | 0,0043 | 16,9 | 0,0683 | sim |
| banco_de_dados | zstd-1 -> zstd-3 | 2,23 | 0,0308 | 0,0000 | 4.178 | 0,0308 | sim |
| banco_de_dados | zstd-3 -> zstd-9 | 1,32 | 0,0182 | 0,0003 | 53,5 | 0,0179 | sim |
| banco_de_dados | zstd-9 -> zstd-19 | 1,04 | 0,0144 | 0,0034 | 4,2 | 0,0110 | sim |
| banco_de_dados | zstd-19 -> xz-9e | 2,82 | 0,0389 | 0,0025 | 15,6 | 0,0364 | sim |
| xml | zstd-1 -> zstd-3 | 0,91 | 0,0125 | 0,0000 | 4.236 | 0,0125 | sim |
| xml | zstd-3 -> zstd-9 | 2,10 | 0,0289 | 0,0001 | 274,9 | 0,0288 | sim |
| xml | zstd-9 -> zstd-19 | 1,04 | 0,0143 | 0,0055 | 2,6 | 0,0088 | sim |
| xml | zstd-19 -> xz-9e | 0,40 | 0,0055 | 0,0001 | 51,8 | 0,0054 | sim |
| dna | zstd-1 -> zstd-3 | 0,22 | 0,0031 | 0,0000 | 27.405 | 0,0031 | sim |
| dna | zstd-3 -> zstd-9 | 1,36 | 0,0188 | 0,0006 | 31,5 | 0,0182 | sim |
| dna | zstd-9 -> zstd-19 | 3,83 | 0,0529 | 0,0064 | 8,3 | 0,0465 | sim |
| dna | zstd-19 -> xz-9e | -1,32 | -0,0183 | 0,0025 | -7,2 | -0,0208 | nao |
| sint_logs | zstd-1 -> zstd-3 | -0,24 | -0,0033 | 0,0000 | -2.639 | -0,0033 | nao |
| sint_logs | zstd-3 -> zstd-9 | 2,90 | 0,0400 | 0,0004 | 108,9 | 0,0397 | sim |
| sint_logs | zstd-9 -> zstd-19 | 3,16 | 0,0437 | 0,0045 | 9,8 | 0,0392 | sim |
| sint_logs | zstd-19 -> xz-9e | 0,18 | 0,0025 | 0,0021 | 1,2 | 0,0005 | sim |
| sint_incompressivel | zstd-1 -> zstd-3 | 0,00 | 0,0000 | -0,0000 | - | 0,0000 | sim |
| sint_incompressivel | zstd-3 -> zstd-9 | 0,00 | 0,0000 | 0,0000 | 0,0 | -0,0000 | nao |
| sint_incompressivel | zstd-9 -> zstd-19 | 0,00 | 0,0000 | 0,0007 | 0,0 | -0,0007 | nao |
| sint_incompressivel | zstd-19 -> xz-9e | 0,00 | 0,0000 | 0,0020 | 0,0 | -0,0020 | nao |
| sint_ja_comprimido | zstd-1 -> zstd-3 | 0,00 | 0,0000 | 0,0000 | 0,0 | -0,0000 | nao |
| sint_ja_comprimido | zstd-3 -> zstd-9 | 0,00 | 0,0000 | 0,0000 | 0,0 | -0,0000 | nao |
| sint_ja_comprimido | zstd-9 -> zstd-19 | 0,00 | 0,0000 | 0,0007 | 0,0 | -0,0007 | nao |
| sint_ja_comprimido | zstd-19 -> xz-9e | 0,00 | 0,0000 | 0,0020 | 0,0 | -0,0020 | nao |
| mix_real | zstd-1 -> zstd-3 | 3,63 | 0,0501 | 0,0000 | 3.118 | 0,0501 | sim |
| mix_real | zstd-3 -> zstd-9 | 2,63 | 0,0363 | 0,0004 | 97,9 | 0,0360 | sim |
| mix_real | zstd-9 -> zstd-19 | 2,37 | 0,0328 | 0,0040 | 8,3 | 0,0288 | sim |
| mix_real | zstd-19 -> xz-9e | 2,27 | 0,0313 | 0,0030 | 10,5 | 0,0283 | sim |


## 11. Regra de recompressao em camadas (modo bloco_1MiB)

Dado ja em zstd-1 ou zstd-3 na ingestao. Meses de vida restante minimos para o reempacotamento se pagar: M* = (custo de descompressao da origem + custo de compressao do destino) / (ganho mensal de armazenamento - taxa de leitura x aumento do custo de leitura). 'nunca' = o custo de leitura extra supera o ganho de armazenamento. CPU de reempacotamento com eficiencia de escala de servidor cheio (N=32).

| classe | cenario | camada | reempacotar | delta economia (pp) | custo repack US$/GB | M* (meses restantes) |
|---|---|---|---|---|---|---|
| texto | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 6,4 | 0,0060 | nunca |
| texto | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 6,4 | 0,0060 | 25,45 |
| texto | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 6,4 | 0,0060 | 23,57 |
| texto | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 21,07 |
| texto | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 21,33 |
| texto | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 21,35 |
| texto | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 6,4 | 0,0060 | 5,54 |
| texto | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 6,4 | 0,0060 | 4,15 |
| texto | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 6,4 | 0,0060 | 4,09 |
| texto | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 3,70 |
| texto | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 3,71 |
| texto | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 6,0 | 0,0051 | 3,71 |
| texto | onprem+onprem | quente | zstd-3 -> xz-9e | 6,4 | 0,0018 | 158,23 |
| texto | onprem+onprem | morno | zstd-3 -> xz-9e | 6,4 | 0,0018 | 14,93 |
| texto | onprem+onprem | frio | zstd-3 -> xz-9e | 6,4 | 0,0018 | 14,26 |
| texto | onprem+onprem | quente | zstd-3 -> zstd-19 | 6,0 | 0,0015 | 12,82 |
| texto | onprem+onprem | morno | zstd-3 -> zstd-19 | 6,0 | 0,0015 | 12,92 |
| texto | onprem+onprem | frio | zstd-3 -> zstd-19 | 6,0 | 0,0015 | 12,92 |
| codigo | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 3,5 | 0,0047 | nunca |
| codigo | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 3,5 | 0,0047 | 40,42 |
| codigo | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 3,5 | 0,0047 | 33,60 |
| codigo | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 29,57 |
| codigo | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 29,34 |
| codigo | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 29,33 |
| codigo | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 3,5 | 0,0047 | 14,46 |
| codigo | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 3,5 | 0,0047 | 6,00 |
| codigo | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 3,5 | 0,0047 | 5,83 |
| codigo | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 5,11 |
| codigo | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 5,10 |
| codigo | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 3,0 | 0,0036 | 5,10 |
| codigo | onprem+onprem | quente | zstd-3 -> xz-9e | 3,5 | 0,0014 | nunca |
| codigo | onprem+onprem | morno | zstd-3 -> xz-9e | 3,5 | 0,0014 | 22,62 |
| codigo | onprem+onprem | frio | zstd-3 -> xz-9e | 3,5 | 0,0014 | 20,31 |
| codigo | onprem+onprem | quente | zstd-3 -> zstd-19 | 3,0 | 0,0011 | 17,84 |
| codigo | onprem+onprem | morno | zstd-3 -> zstd-19 | 3,0 | 0,0011 | 17,76 |
| codigo | onprem+onprem | frio | zstd-3 -> zstd-19 | 3,0 | 0,0011 | 17,76 |
| binario | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 7,0 | 0,0042 | nunca |
| binario | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 7,0 | 0,0042 | 17,53 |
| binario | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 7,0 | 0,0042 | 15,00 |
| binario | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 15,67 |
| binario | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 14,76 |
| binario | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 14,71 |
| binario | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 7,0 | 0,0042 | 5,31 |
| binario | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 7,0 | 0,0042 | 2,67 |
| binario | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 7,0 | 0,0042 | 2,60 |
| binario | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 2,59 |
| binario | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 2,56 |
| binario | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 4,9 | 0,0029 | 2,56 |
| binario | onprem+onprem | quente | zstd-3 -> xz-9e | 7,0 | 0,0013 | nunca |
| binario | onprem+onprem | morno | zstd-3 -> xz-9e | 7,0 | 0,0013 | 9,94 |
| binario | onprem+onprem | frio | zstd-3 -> xz-9e | 7,0 | 0,0013 | 9,07 |
| binario | onprem+onprem | quente | zstd-3 -> zstd-19 | 4,9 | 0,0009 | 9,25 |
| binario | onprem+onprem | morno | zstd-3 -> zstd-19 | 4,9 | 0,0009 | 8,92 |
| binario | onprem+onprem | frio | zstd-3 -> zstd-19 | 4,9 | 0,0009 | 8,91 |
| cientifico | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 10,9 | 0,0060 | nunca |
| cientifico | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 10,9 | 0,0060 | 15,03 |
| cientifico | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 10,9 | 0,0060 | 13,77 |
| cientifico | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 17,55 |
| cientifico | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 17,30 |
| cientifico | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 17,29 |
| cientifico | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 10,9 | 0,0060 | 3,40 |
| cientifico | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 10,9 | 0,0060 | 2,43 |
| cientifico | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 10,9 | 0,0060 | 2,39 |
| cientifico | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 3,02 |
| cientifico | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 3,01 |
| cientifico | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 5,6 | 0,0039 | 3,01 |
| cientifico | onprem+onprem | quente | zstd-3 -> xz-9e | 10,9 | 0,0018 | nunca |
| cientifico | onprem+onprem | morno | zstd-3 -> xz-9e | 10,9 | 0,0018 | 8,77 |
| cientifico | onprem+onprem | frio | zstd-3 -> xz-9e | 10,9 | 0,0018 | 8,33 |
| cientifico | onprem+onprem | quente | zstd-3 -> zstd-19 | 5,6 | 0,0012 | 10,56 |
| cientifico | onprem+onprem | morno | zstd-3 -> zstd-19 | 5,6 | 0,0012 | 10,47 |
| cientifico | onprem+onprem | frio | zstd-3 -> zstd-19 | 5,6 | 0,0012 | 10,47 |
| banco_de_dados | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 5,2 | 0,0046 | nunca |
| banco_de_dados | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 5,2 | 0,0046 | 25,33 |
| banco_de_dados | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 5,2 | 0,0046 | 22,04 |
| banco_de_dados | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 39,29 |
| banco_de_dados | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 39,23 |
| banco_de_dados | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 39,23 |
| banco_de_dados | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 5,2 | 0,0046 | 7,07 |
| banco_de_dados | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 5,2 | 0,0046 | 3,91 |
| banco_de_dados | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 5,2 | 0,0046 | 3,82 |
| banco_de_dados | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 6,82 |
| banco_de_dados | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 6,82 |
| banco_de_dados | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 2,4 | 0,0037 | 6,82 |
| banco_de_dados | onprem+onprem | quente | zstd-3 -> xz-9e | 5,2 | 0,0014 | nunca |
| banco_de_dados | onprem+onprem | morno | zstd-3 -> xz-9e | 5,2 | 0,0014 | 14,46 |
| banco_de_dados | onprem+onprem | frio | zstd-3 -> xz-9e | 5,2 | 0,0014 | 13,33 |
| banco_de_dados | onprem+onprem | quente | zstd-3 -> zstd-19 | 2,4 | 0,0011 | 23,77 |
| banco_de_dados | onprem+onprem | morno | zstd-3 -> zstd-19 | 2,4 | 0,0011 | 23,75 |
| banco_de_dados | onprem+onprem | frio | zstd-3 -> zstd-19 | 2,4 | 0,0011 | 23,75 |
| xml | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 3,5 | 0,0052 | nunca |
| xml | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 3,5 | 0,0052 | 38,67 |
| xml | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 3,5 | 0,0052 | 36,59 |
| xml | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 42,31 |
| xml | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 44,08 |
| xml | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 44,17 |
| xml | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 3,5 | 0,0052 | 7,85 |
| xml | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 3,5 | 0,0052 | 6,42 |
| xml | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 3,5 | 0,0052 | 6,36 |
| xml | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 7,62 |
| xml | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 7,68 |
| xml | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 3,1 | 0,0055 | 7,68 |
| xml | onprem+onprem | quente | zstd-3 -> xz-9e | 3,5 | 0,0016 | 65,59 |
| xml | onprem+onprem | morno | zstd-3 -> xz-9e | 3,5 | 0,0016 | 22,88 |
| xml | onprem+onprem | frio | zstd-3 -> xz-9e | 3,5 | 0,0016 | 22,14 |
| xml | onprem+onprem | quente | zstd-3 -> zstd-19 | 3,1 | 0,0017 | 26,05 |
| xml | onprem+onprem | morno | zstd-3 -> zstd-19 | 3,1 | 0,0017 | 26,71 |
| xml | onprem+onprem | frio | zstd-3 -> zstd-19 | 3,1 | 0,0017 | 26,74 |
| dna | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 3,9 | 0,0082 | nunca |
| dna | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 3,9 | 0,0082 | 61,10 |
| dna | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 3,9 | 0,0082 | 53,20 |
| dna | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 31,31 |
| dna | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 33,27 |
| dna | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 33,38 |
| dna | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 3,9 | 0,0082 | 17,01 |
| dna | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 3,9 | 0,0082 | 9,44 |
| dna | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 3,9 | 0,0082 | 9,23 |
| dna | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 5,74 |
| dna | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 5,80 |
| dna | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 5,2 | 0,0069 | 5,81 |
| dna | onprem+onprem | quente | zstd-3 -> xz-9e | 3,9 | 0,0025 | nunca |
| dna | onprem+onprem | morno | zstd-3 -> xz-9e | 3,9 | 0,0025 | 34,90 |
| dna | onprem+onprem | frio | zstd-3 -> xz-9e | 3,9 | 0,0025 | 32,17 |
| dna | onprem+onprem | quente | zstd-3 -> zstd-19 | 5,2 | 0,0021 | 19,43 |
| dna | onprem+onprem | morno | zstd-3 -> zstd-19 | 5,2 | 0,0021 | 20,17 |
| dna | onprem+onprem | frio | zstd-3 -> zstd-19 | 5,2 | 0,0021 | 20,21 |
| sint_logs | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 6,3 | 0,0056 | nunca |
| sint_logs | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 6,3 | 0,0056 | 24,53 |
| sint_logs | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 6,3 | 0,0056 | 22,58 |
| sint_logs | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 18,77 |
| sint_logs | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 19,85 |
| sint_logs | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 19,91 |
| sint_logs | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 6,3 | 0,0056 | 5,46 |
| sint_logs | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 6,3 | 0,0056 | 3,98 |
| sint_logs | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 6,3 | 0,0056 | 3,92 |
| sint_logs | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 3,43 |
| sint_logs | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 3,46 |
| sint_logs | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 6,1 | 0,0048 | 3,46 |
| sint_logs | onprem+onprem | quente | zstd-3 -> xz-9e | 6,3 | 0,0017 | 747,94 |
| sint_logs | onprem+onprem | morno | zstd-3 -> xz-9e | 6,3 | 0,0017 | 14,35 |
| sint_logs | onprem+onprem | frio | zstd-3 -> xz-9e | 6,3 | 0,0017 | 13,66 |
| sint_logs | onprem+onprem | quente | zstd-3 -> zstd-19 | 6,1 | 0,0015 | 11,62 |
| sint_logs | onprem+onprem | morno | zstd-3 -> zstd-19 | 6,1 | 0,0015 | 12,03 |
| sint_logs | onprem+onprem | frio | zstd-3 -> zstd-19 | 6,1 | 0,0015 | 12,05 |
| sint_incompressivel | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_incompressivel | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_incompressivel | onprem+onprem | quente | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_incompressivel | onprem+onprem | morno | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_incompressivel | onprem+onprem | frio | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_incompressivel | onprem+onprem | quente | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| sint_incompressivel | onprem+onprem | morno | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| sint_incompressivel | onprem+onprem | frio | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 0,0 | 0,0027 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 0,0 | 0,0007 | nunca |
| sint_ja_comprimido | onprem+onprem | quente | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_ja_comprimido | onprem+onprem | morno | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_ja_comprimido | onprem+onprem | frio | zstd-3 -> xz-9e | 0,0 | 0,0008 | nunca |
| sint_ja_comprimido | onprem+onprem | quente | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| sint_ja_comprimido | onprem+onprem | morno | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| sint_ja_comprimido | onprem+onprem | frio | zstd-3 -> zstd-19 | 0,0 | 0,0002 | nunca |
| mix_real | nuvem+glacier_ir | quente | zstd-3 -> xz-9e | 7,3 | 0,0056 | nunca |
| mix_real | nuvem+glacier_ir | morno | zstd-3 -> xz-9e | 7,3 | 0,0056 | 21,19 |
| mix_real | nuvem+glacier_ir | frio | zstd-3 -> xz-9e | 7,3 | 0,0056 | 19,17 |
| mix_real | nuvem+glacier_ir | quente | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 21,49 |
| mix_real | nuvem+glacier_ir | morno | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 21,39 |
| mix_real | nuvem+glacier_ir | frio | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 21,38 |
| mix_real | nuvem+s3_standard | quente | zstd-3 -> xz-9e | 7,3 | 0,0056 | 5,03 |
| mix_real | nuvem+s3_standard | morno | zstd-3 -> xz-9e | 7,3 | 0,0056 | 3,38 |
| mix_real | nuvem+s3_standard | frio | zstd-3 -> xz-9e | 7,3 | 0,0056 | 3,33 |
| mix_real | nuvem+s3_standard | quente | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 3,72 |
| mix_real | nuvem+s3_standard | morno | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 3,72 |
| mix_real | nuvem+s3_standard | frio | zstd-3 -> zstd-19 | 5,0 | 0,0043 | 3,72 |
| mix_real | onprem+onprem | quente | zstd-3 -> xz-9e | 7,3 | 0,0017 | nunca |
| mix_real | onprem+onprem | morno | zstd-3 -> xz-9e | 7,3 | 0,0017 | 12,31 |
| mix_real | onprem+onprem | frio | zstd-3 -> xz-9e | 7,3 | 0,0017 | 11,59 |
| mix_real | onprem+onprem | quente | zstd-3 -> zstd-19 | 5,0 | 0,0013 | 12,98 |
| mix_real | onprem+onprem | morno | zstd-3 -> zstd-19 | 5,0 | 0,0013 | 12,95 |
| mix_real | onprem+onprem | frio | zstd-3 -> zstd-19 | 5,0 | 0,0013 | 12,94 |


## 12. Memoria (pico de RSS, entrada de 32 MiB de mozilla)

| config | RSS compressao MB | RSS descompressao MB | comprimido (bytes) |
|---|---|---|---|
| zstd-3 | 43,5 | 6,8 | 12412030 |
| zstd-9 | 54,8 | 9,4 | 11460917 |
| zstd-19 | 130,7 | 13,5 | 10448311 |
| zstd-22 | 441,9 | 37,8 | 10391324 |
| zstd-3-long27 | 42,7 | 37,8 | 12384637 |
| xz-9e | 363,2 | 35,1 | 9424812 |


## 13. P-core vs E-core (subconjunto dickens+mozilla, 1 nucleo isolado)

| modo | config | P comp | E comp | E/P comp | P decomp | E decomp | E/P decomp |
|---|---|---|---|---|---|---|---|
| bloco_1MiB | xz-6 | 7 | 5 | 0,69 | 132 | 92 | 0,70 |
| bloco_1MiB | zlib-6 | 40 | 23 | 0,57 | 397 | 303 | 0,76 |
| bloco_1MiB | zstd-1 | 607 | 359 | 0,59 | 1.729 | 1.069 | 0,62 |
| bloco_1MiB | zstd-19 | 9 | 5 | 0,61 | 1.773 | 1.073 | 0,61 |
| bloco_1MiB | zstd-3 | 395 | 232 | 0,59 | 1.650 | 998 | 0,61 |
| bloco_1MiB | zstd-9 | 107 | 59 | 0,55 | 2.126 | 1.225 | 0,58 |
| inteiro | xz-6 | 5 | 3 | 0,71 | 139 | 97 | 0,70 |
| inteiro | zlib-6 | 40 | 23 | 0,57 | 388 | 294 | 0,76 |
| inteiro | zstd-1 | 616 | 364 | 0,59 | 1.395 | 859 | 0,62 |
| inteiro | zstd-19 | 6 | 4 | 0,69 | 1.192 | 758 | 0,64 |
| inteiro | zstd-3 | 387 | 231 | 0,60 | 1.192 | 700 | 0,59 |
| inteiro | zstd-9 | 102 | 55 | 0,54 | 1.532 | 911 | 0,59 |


### Vies de concorrencia da medicao principal (4 trabalhadores) vs P-core isolado, mesmas celulas

| modo | config | comp paralelo/isolado | decomp paralelo/isolado |
|---|---|---|---|
| bloco_1MiB | zlib-6 | 0,913 | 0,956 |
| bloco_1MiB | zstd-1 | 0,936 | 1,081 |
| bloco_1MiB | zstd-19 | 0,889 | 0,921 |
| bloco_1MiB | zstd-3 | 0,929 | 1,035 |
| bloco_1MiB | zstd-9 | 0,889 | 0,873 |
| inteiro | xz-6 | 0,766 | 0,908 |
| inteiro | zlib-6 | 0,916 | 0,987 |
| inteiro | zstd-1 | 0,955 | 1,256 |
| inteiro | zstd-19 | 0,786 | 0,931 |
| inteiro | zstd-3 | 0,919 | 1,089 |
| inteiro | zstd-9 | 0,851 | 1,094 |


## 14. Conferencia cruzada com `zstd -b` (CLI, mesma fatia de 8 MiB, P-core CPU 0)

| arquivo | nivel | modo | CLI comp MB/s | CLI decomp MB/s | harness comp MB/s | harness decomp MB/s | comp harness/CLI | decomp harness/CLI | bytes comprimidos CLI vs harness |
|---|---|---|---|---|---|---|---|---|---|
| dickens | 1 | inteiro | 418 | 2.010 | 471 | 1.288 | 1,13 | 0,64 | 3496747 vs 3495986 |
| dickens | 3 | inteiro | 286 | 1.752 | 304 | 1.062 | 1,06 | 0,61 | 3010565 vs 3010565 |
| dickens | 3 | bloco_1MiB | 284 | 1.752 | 303 | 1.447 | 1,07 | 0,83 | 3063426 vs 3063426 |
| mozilla | 1 | inteiro | 799 | 2.715 | 889 | 1.521 | 1,11 | 0,56 | 4869437 vs 4865644 |
| mozilla | 3 | inteiro | 506 | 2.511 | 533 | 1.360 | 1,05 | 0,54 | 4661583 vs 4656376 |
| mozilla | 3 | bloco_1MiB | 543 | 2.516 | 569 | 1.920 | 1,05 | 0,76 | 4696906 vs 4691804 |


## 15. Falhas e violacoes

Violacoes de bit-exatidao: nenhuma (sha256 do original == sha256 do descomprimido em todas as medicoes)

Celulas com falha: nenhuma
