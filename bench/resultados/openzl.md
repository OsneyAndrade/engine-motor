# OpenZL como baseline de compressao (T1.9, fase A: razao e bit-exatidao, sem tempo)

Gerado por `bench/openzl_bench.py relatorio` em 2026-11-04 20:01:19 a partir de `openzl_razao.json`. Imagem `syntra-openzl:pinned`, commit OpenZL `32246b48faee46807f84183dac4db479089f5445`, `Demo CLI for OpenZL. Version 0.3.0`.

VELOCIDADE NAO MEDIDA nesta fase (maquina compartilhada). Violacoes de bit-exatidao: 0. Falhas de execucao: 0.

## Conferencias

- sha256 dos 18 arquivos de entrada contra `medicoes.json`: todos conferem.
- zstd-3 (`compression.zstd`) na fatia de 8 MiB de `medicoes.json`: 18 de 18 arquivos com bytes comprimidos identicos aos de `medicoes.json`.

## Por classe (soma dos bytes das fatias de teste; razao = original / comprimido)

| classe | bytes de teste | zstd-3 | zstd-19 | xz-9e | OZL generico L6 | OZL generico (melhor de L6/L9) | OZL a priori | OZL melhor | melhor vs zstd-3 | melhor vs zstd-19 | melhor vs xz-9e | a priori vs zstd-3 | a priori vs zstd-19 | a priori vs xz-9e |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| banco_de_dados | 5042844 | 2.878 | 3.206 | 3.501 | 2.940 | 2.981 | 3.010 | 3.010 | +4.6% | -6.1% | -14.0% | +4.6% | -6.1% | -14.0% |
| binario | 11464704 | 2.944 | 3.599 | 4.048 | 2.843 | 2.884 | 3.123 | 3.132 | +6.4% | -13.0% | -22.6% | +6.1% | -13.2% | -22.9% |
| cientifico | 21236957 | 2.519 | 2.956 | 3.363 | 2.653 | 2.700 | 3.813 | 3.813 | +51.3% | +29.0% | +13.4% | +51.3% | +29.0% | +13.4% |
| codigo | 8388605 | 5.876 | 8.171 | 8.912 | 5.759 | 5.880 | 6.970 | 6.970 | +18.6% | -14.7% | -21.8% | +18.6% | -14.7% | -21.8% |
| dna | 2319345 | 3.334 | 4.031 | 3.859 | 3.446 | 3.487 | 3.433 | 3.487 | +4.6% | -13.5% | -9.6% | +3.0% | -14.8% | -11.0% |
| sint_csv_vendas | 16777231 | 4.973 | 7.491 | 7.950 | 5.735 | 5.935 | 17.652 | 17.652 | +255.0% | +135.6% | +122.0% | +255.0% | +135.6% | +122.0% |
| sint_float32_telemetria | 16777216 | 4.382 | 5.733 | 7.030 | 4.788 | 4.896 | 10.769 | 10.769 | +145.8% | +87.8% | +53.2% | +145.8% | +87.8% | +53.2% |
| sint_incompressivel | 8388608 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% |
| sint_int64_timestamps | 8388608 | 6.071 | 6.065 | 6.863 | 3.486 | 3.488 | 9.403 | 9.403 | +54.9% | +55.0% | +37.0% | +54.9% | +55.0% | +37.0% |
| sint_ja_comprimido | 4192940 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% |
| sint_logs | 8388602 | 4.075 | 5.709 | 5.757 | 4.258 | 4.362 | 4.581 | 4.581 | +12.4% | -19.8% | -20.4% | +12.4% | -19.8% | -20.4% |
| sint_ndjson_eventos | 16777118 | 5.876 | 8.660 | 8.807 | 6.328 | 6.533 | 6.954 | 6.954 | +18.3% | -19.7% | -21.0% | +18.3% | -19.7% | -21.0% |
| texto | 20054724 | 3.226 | 4.228 | 4.281 | 3.455 | 3.560 | 3.511 | 3.588 | +11.2% | -15.1% | -16.2% | +8.9% | -16.9% | -18.0% |
| xml | 2050502 | 7.599 | 10.193 | 10.630 | 8.013 | 8.332 | 9.141 | 9.141 | +20.3% | -10.3% | -14.0% | +20.3% | -10.3% | -14.0% |

`OpenZL melhor` e o melhor entre generico, perfil e treinado, escolhido no proprio teste (otimista). `OpenZL a priori` nao olha o teste: treino com perfil estrutural, senao treino `serial`, senao perfil, senao generico L6. Ganho = razao OpenZL / razao do baseline - 1.

## Por arquivo

| arquivo | classe | sintetico | bytes teste | zstd-1 | zstd-3 | zstd-19 | xz-9e | OZL gen. L6 | OZL gen. melhor L6/L9 | OZL perfil | OZL treinado | config a priori | melhor OZL | vs zstd-3 | vs zstd-19 | vs xz-9e |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| dickens | texto | nao | 5096179 | 2.377 | 2.762 | 3.472 | 3.503 | 2.910 | 2.992 | - | 3.072 | treinado:serial | treinado:serial | +11.2% | -11.5% | -12.3% |
| webster | texto | nao | 8388544 | 3.031 | 3.418 | 4.592 | 4.629 | 3.723 | 3.842 | - | - | generico:serial | generico:serial@L9 | +12.4% | -16.3% | -17.0% |
| reymont | texto | nao | 3309722 | 3.077 | 3.383 | 4.706 | 4.825 | 3.610 | 3.737 | - | - | generico:serial | generico:serial@L9 | +10.5% | -20.6% | -22.5% |
| bible.txt | texto | nao | 2023622 | 3.008 | 3.326 | 4.225 | 4.249 | 3.548 | 3.631 | - | - | generico:serial | generico:serial@L9 | +9.2% | -14.1% | -14.5% |
| world192.txt | texto | nao | 1236657 | 3.272 | 3.736 | 4.636 | 4.838 | 3.964 | 4.069 | - | - | generico:serial | generico:serial@L9 | +8.9% | -12.2% | -15.9% |
| samba | codigo | nao | 8388605 | 5.386 | 5.876 | 8.171 | 8.912 | 5.759 | 5.880 | - | 6.970 | treinado:serial | treinado:serial | +18.6% | -14.7% | -21.8% |
| mozilla | binario | nao | 8388608 | 3.061 | 3.413 | 4.238 | 4.879 | 3.254 | 3.313 | - | 3.783 | treinado:serial | treinado:serial | +10.8% | -10.7% | -22.5% |
| ooffice | binario | nao | 3076096 | 1.862 | 2.141 | 2.550 | 2.765 | 2.116 | 2.132 | - | - | generico:serial | generico:serial@L9 | -0.4% | -16.4% | -22.9% |
| sao | cientifico | nao | 3625972 | 1.148 | 1.305 | 1.444 | 1.628 | 1.364 | 1.375 | 2.011 | 3.072 | treinado:sao | treinado:sao | +135.5% | +112.8% | +88.7% |
| nci | cientifico | nao | 8388583 | 12.341 | 12.322 | 20.480 | 22.499 | 12.879 | 13.238 | - | 15.727 | treinado:serial | treinado:serial | +27.6% | -23.2% | -30.1% |
| x-ray | cientifico | nao | 4237120 | 1.233 | 1.364 | 1.628 | 1.881 | 1.451 | 1.490 | 1.644 | - | perfil:le-u16 | perfil:le-u16 | +20.6% | +1.0% | -12.6% |
| mr | cientifico | nao | 4985282 | 2.502 | 2.676 | 3.000 | 3.409 | 2.807 | 2.847 | 3.010 | 3.896 | treinado:le-u16 | treinado:le-u16 | +45.6% | +29.9% | +14.3% |
| osdb | banco_de_dados | nao | 5042844 | 2.701 | 2.878 | 3.206 | 3.501 | 2.940 | 2.981 | - | 3.010 | treinado:serial | treinado:serial | +4.6% | -6.1% | -14.0% |
| xml | xml | nao | 2050502 | 6.747 | 7.599 | 10.193 | 10.630 | 8.013 | 8.332 | - | 9.141 | treinado:serial | treinado:serial | +20.3% | -10.3% | -14.0% |
| E.coli | dna | nao | 2319345 | 3.292 | 3.334 | 4.031 | 3.859 | 3.446 | 3.487 | 3.433 | - | perfil:u8 | generico:serial@L9 | +4.6% | -13.5% | -9.6% |
| aleatorio.bin | sint_incompressivel | SIM | 8388608 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | - | - | generico:serial | generico:serial | +0.0% | +0.0% | +0.0% |
| logs.log | sint_logs | SIM | 8388602 | 4.095 | 4.075 | 5.709 | 5.757 | 4.258 | 4.362 | - | 4.581 | treinado:serial | treinado:serial | +12.4% | -19.8% | -20.4% |
| ja_comprimido.xz | sint_ja_comprimido | SIM | 4192940 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | - | - | generico:serial | generico:serial | +0.0% | +0.0% | +0.0% |
| sint_vendas.csv | sint_csv_vendas | SIM | 16777231 | 4.684 | 4.973 | 7.491 | 7.950 | 5.735 | 5.935 | 6.569 | 17.652 | treinado:csv | treinado:csv | +255.0% | +135.6% | +122.0% |
| sint_eventos.ndjson | sint_ndjson_eventos | SIM | 16777118 | 6.222 | 5.876 | 8.660 | 8.807 | 6.328 | 6.533 | - | 6.954 | treinado:serial | treinado:serial | +18.3% | -19.7% | -21.0% |
| sint_telemetria_f32.bin | sint_float32_telemetria | SIM | 16777216 | 3.413 | 4.382 | 5.733 | 7.030 | 4.788 | 4.896 | 9.855 | 10.769 | treinado:le-u32 | treinado:le-u32 | +145.8% | +87.8% | +53.2% |
| sint_timestamps_i64.bin | sint_int64_timestamps | SIM | 8388608 | 6.071 | 6.071 | 6.065 | 6.863 | 3.486 | 3.488 | 9.330 | 9.403 | treinado:le-i64 | treinado:le-i64 | +54.9% | +55.0% | +37.0% |

## Onde o OpenZL perde ou nao usa codecs estruturais

| arquivo | melhor OpenZL perde para | configs sem nenhum codec estrutural no grafo resolvido (trace) | configs em que --strict falhou |
|---|---|---|---|
| dickens | zstd-19, xz-9e | treinado:serial | - |
| webster | zstd-19, xz-9e | - | - |
| reymont | zstd-19, xz-9e | - | - |
| bible.txt | zstd-19, xz-9e | - | - |
| world192.txt | zstd-19, xz-9e | - | - |
| samba | zstd-19, xz-9e | treinado:serial | - |
| mozilla | zstd-19, xz-9e | treinado:serial | - |
| ooffice | zstd-3, zstd-19, xz-9e | - | - |
| sao | - | - | - |
| nci | zstd-19, xz-9e | treinado:serial | - |
| x-ray | xz-9e | - | - |
| mr | - | - | - |
| osdb | zstd-19, xz-9e | treinado:serial | - |
| xml | zstd-19, xz-9e | treinado:serial | - |
| E.coli | zstd-19, xz-9e | - | - |
| aleatorio.bin | - | - | - |
| logs.log | zstd-19, xz-9e | treinado:serial | - |
| ja_comprimido.xz | - | - | - |
| sint_vendas.csv | - | - | - |
| sint_eventos.ndjson | zstd-19, xz-9e | treinado:serial | - |
| sint_telemetria_f32.bin | - | - | - |
| sint_timestamps_i64.bin | - | - | - |

## Treinos

| arquivo | treino | rc | relogio (s) | limite (s) | bytes do compressor | ganho no treino (%, in-sample, reportado pelo trainer) |
|---|---|---|---|---|---|---|
| dickens | treinado:serial | 0 | 77.6 | 360 | 564 | 4.39 |
| samba | treinado:serial | 0 | 297.8 | 360 | 564 | 3.96 |
| mozilla | treinado:serial | 0 | 139.0 | 360 | 564 | 4.49 |
| sao | treinado:sao | 0 | 91.0 | 360 | 4997 | 65.09 |
| sao | treinado:sddl2(sao_silesia.sddl) | 0 | 89.4 | 360 | 3495 | 74.76 |
| nci | treinado:serial | 0 | 62.8 | 360 | 564 | 19.71 |
| mr | treinado:le-u16 | 0 | 63.4 | 360 | 1189 | 28.47 |
| osdb | treinado:serial | 0 | 63.1 | 360 | 564 | 2.23 |
| xml | treinado:serial | 0 | 30.0 | 360 | 564 | 21.32 |
| logs.log | treinado:serial | 0 | 364.4 | 360 (atingido) | 564 | 8.21 |
| sint_vendas.csv | treinado:csv | 0 | 188.3 | 360 | 9567 | 161.95 |
| sint_eventos.ndjson | treinado:serial | 0 | 367.3 | 360 (atingido) | 564 | 10.08 |
| sint_telemetria_f32.bin | treinado:le-u32 | 0 | 215.3 | 360 | 1051 | 71.03 |
| sint_timestamps_i64.bin | treinado:le-i64 | 0 | 119.9 | 360 | 721 | 14.41 |

### Determinismo do treino

`sao` perfil `sao`, rodadas repetidas sobre as mesmas amostras:

| sha256 do compressor | bytes do compressor | bytes comprimidos no teste | razao no teste |
|---|---|---|---|
| 09044550cbea77d2 | 5034 | 1180895 | 3.0705 |
| c11d3de44418bfda | 5148 | 1131803 | 3.2037 |

`sao` perfil `sao`, rodadas repetidas (`openzl_determinismo_treino_1thread.json`):

| sha256 do compressor | bytes do compressor | bytes comprimidos no teste | razao no teste |
|---|---|---|---|
| 2d80db4de7002fcd | 5055 | 1132228 | 3.2025 |
| 5f31ea93a7e99217 | 4747 | 1182312 | 3.0668 |

## Quadro (frame) e entradas pequenas (`openzl_bench.py quadro`)

Entrada incompressivel (fatia de `aleatorio.bin`), perfil `serial`, nivel 6:

| entrada (B) | quadro (B) | sobrecarga (B) | versao do formato | flags | primeiros 16 bytes (hex) | bit-exato |
|---|---|---|---|---|---|---|
| 0 | 20 | 20 | 27 | 3 | dba5b1d703010102010100c294d3383d | True |
| 1 | 21 | 20 | 27 | 3 | dba5b1d7030102e801010159b8e5ad8d | True |
| 16 | 36 | 20 | 27 | 3 | dba5b1d7030111fa01011059ce6148cf | True |
| 256 | 278 | 22 | 27 | 3 | dba5b1d703018102a50101800259ce61 | True |
| 4096 | 4118 | 22 | 27 | 3 | dba5b1d703018120fb0101802059ce61 | True |
| 65536 | 65560 | 24 | 27 | 3 | dba5b1d70301818004cb010180800459 | True |

Corrupcao de 1 bit em quadro de 26158 B (texto de 64 KiB, `serial`): 277 posicoes testadas (bytes 0-63, ultimos 12 bytes, e posicoes espacadas de 130 em 130); detectadas com rc != 0: 277; sem efeito (saida identica): 0; CORRUPCAO SILENCIOSA (rc = 0 e saida diferente): 0. Truncamentos testados: 7, todos com rc != 0: sim.

Blocos independentes (cada bloco comprimido sozinho; razao = soma dos bytes originais / soma dos quadros; OpenZL sem treino):

| arquivo | bloco (B) | n blocos | bytes | zstd-3 | zstd-19 | xz-9e | OZL serial L6 | OZL perfil (sem treino) | serial vs zstd-3 |
|---|---|---|---|---|---|---|---|---|---|
| dickens | 65536 | 16 | 1048994 | 2.414 | 2.642 | 2.659 | 2.487 | - | +3.0% |
| dickens | 1048576 | 4 | 4194477 | 2.718 | 3.216 | 3.245 | 2.842 | - | +4.5% |
| samba | 65536 | 16 | 1050416 | 4.835 | 5.377 | 5.536 | 4.794 | - | -0.8% |
| samba | 1048576 | 7 | 7340255 | 5.694 | 7.843 | 8.588 | 5.670 | - | -0.4% |
| xml | 65536 | 16 | 1573159 | 6.687 | 8.102 | 8.279 | 6.848 | - | +2.4% |
| xml | 1048576 | 1 | 1048579 | 12.733 | 17.847 | 18.790 | 12.855 | - | +1.0% |
| sao | 65536 | 16 | 1048320 | 1.244 | 1.385 | 1.516 | 1.268 | sao=1.800 | +1.9% |
| sao | 1048576 | 3 | 3145716 | 1.304 | 1.444 | 1.618 | 1.358 | sao=1.955 | +4.2% |
| mr | 65536 | 16 | 1048576 | 2.660 | 2.789 | 3.284 | 2.716 | le-u16=3.127 | +2.1% |
| mr | 1048576 | 4 | 4194304 | 2.687 | 2.930 | 3.373 | 2.795 | le-u16=3.021 | +4.0% |
| logs.log | 65536 | 16 | 1049884 | 3.903 | 4.571 | 4.591 | 3.901 | - | -0.1% |
| logs.log | 1048576 | 7 | 7340469 | 4.046 | 5.361 | 5.418 | 4.224 | - | +4.4% |
| sint_vendas.csv | 65536 | 16 | 1049121 | 4.372 | 5.003 | 5.276 | 4.621 | csv=5.477 | +5.7% |
| sint_vendas.csv | 1048576 | 8 | 8388825 | 4.828 | 6.407 | 6.806 | 5.449 | csv=6.265 | +12.9% |
| sint_eventos.ndjson | 65536 | 16 | 1049860 | 5.970 | 7.534 | 7.359 | 5.907 | - | -1.0% |
| sint_eventos.ndjson | 1048576 | 8 | 8389288 | 5.948 | 8.259 | 8.320 | 6.416 | - | +7.9% |
| sint_telemetria_f32.bin | 65536 | 16 | 1048576 | 2.839 | 3.390 | 4.468 | 2.969 | le-u32=4.369 | +4.6% |
| sint_telemetria_f32.bin | 1048576 | 8 | 8388608 | 4.636 | 6.056 | 7.481 | 5.287 | le-u32=6.453 | +14.0% |
| sint_timestamps_i64.bin | 65536 | 16 | 1048576 | 6.046 | 6.042 | 6.688 | 3.653 | le-i64=7.941 | -39.6% |
| sint_timestamps_i64.bin | 1048576 | 7 | 7340032 | 6.070 | 6.065 | 6.839 | 3.625 | le-i64=8.199 | -40.3% |

## Baseline com as transformadas do motor (`openzl_bench.py transformadas`)

emulacao em Python das transformadas Delta, ByteSplit e CsvColumnar de src/transform.rs (semantica lida do codigo; INFERENCIA PROPRIA de que equivalem em bytes) seguidas de zstd-19, zstd-22 ou xz-9e; mesma fatia de teste; sha256 do original conferido apos inverter.

| arquivo | bytes teste | zstd-19 puro | xz-9e puro | melhor transformada+codec | razao | config OZL a priori | razao OZL a priori | OZL a priori vs transformada+codec | melhor OZL | melhor OZL vs transformada+codec |
|---|---|---|---|---|---|---|---|---|---|---|
| sint_vendas.csv | 16777231 | 7.491 | 7.950 | zstd-22+csv | 8.365 | treinado:csv | 17.652 | +111.0% | 17.652 | +111.0% |
| sint_telemetria_f32.bin | 16777216 | 5.733 | 7.030 | zstd-19+delta | 7.475 | treinado:le-u32 | 10.769 | +44.1% | 10.769 | +44.1% |
| sint_timestamps_i64.bin | 8388608 | 6.065 | 6.863 | zstd-19+delta>split | 9.325 | treinado:le-i64 | 9.403 | +0.8% | 9.403 | +0.8% |
| mr | 4985282 | 3.000 | 3.409 | xz-9e+split | 3.675 | treinado:le-u16 | 3.896 | +6.0% | 3.896 | +6.0% |
| x-ray | 4237120 | 1.628 | 1.881 | xz-9e+split | 2.081 | perfil:le-u16 | 1.644 | -21.0% | 1.644 | -21.0% |

Violacoes de bit-exatidao nesta secao: 0.

## Referencias publicadas (apenas rotuladas; nao usadas para concluir)

- doc/mkdocs/doc/index.md do repositorio no commit fixado (tabela SAO, Silesia) | dado: sao | zstd-3=1.31, xz-9=1.64, openzl=2.06 | REFERENCIA PUBLICADA, nao usada para concluir; arquivo inteiro, perfil sao
- arXiv:2510.03203, tabela 5 (texto lido localmente) | dado: enwik7 (10^7 bytes de enwik) | zstd-3=2.8, zstd-19=3.58, xz-9=3.68, openzl=3.04 | REFERENCIA PUBLICADA, nao usada para concluir; o artigo diz que o compressor treinado usa zstd-6 em texto
- doc/mkdocs/doc/blog/posts/2026-09-24-compression-transformer.md (v0.3.0) | dado: 868 familias numericas, media geometrica ponderada | zstd-19=5.752, xz-9=5.92, openzl_nao_treinado=6.036, openzl_treinado=7.846, openzl_transformer_nivel7=7.76 | REFERENCIA PUBLICADA, nao usada para concluir; dados numericos reais de varias origens, nao os sinteticos daqui

## Tempo

NAO MEDIDO: fase B pendente, depende de janela exclusiva da maquina (`openzl_bench.py tempo`).
