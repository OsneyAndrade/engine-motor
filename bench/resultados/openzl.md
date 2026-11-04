# OpenZL como baseline de compressao (T1.9, fase A: razao e bit-exatidao, sem tempo)

Gerado por `bench/openzl_bench.py relatorio` em 2026-11-04 17:13:05 a partir de `openzl_razao.json`. Imagem `syntra-openzl:pinned`, commit OpenZL `32246b48faee46807f84183dac4db479089f5445`, `Demo CLI for OpenZL. Version 0.3.0`.

VELOCIDADE NAO MEDIDA nesta fase (maquina compartilhada). Violacoes de bit-exatidao: 0. Falhas de execucao: 0.

## Conferencias

- sha256 dos 18 arquivos de entrada contra `medicoes.json`: todos conferem.
- zstd-3 (`compression.zstd`) na fatia de 8 MiB de `medicoes.json`: 18 de 18 arquivos com bytes comprimidos identicos aos de `medicoes.json`.

## Por classe (soma dos bytes das fatias de teste; razao = original / comprimido)

| classe | bytes de teste | zstd-3 | zstd-19 | xz-9e | OZL generico L6 | OZL generico (melhor de L6/L9) | OZL melhor | ganho vs zstd-3 | vs zstd-19 | vs xz-9e |
|---|---|---|---|---|---|---|---|---|---|---|
| banco_de_dados | 5042844 | 2.878 | 3.206 | 3.501 | 2.940 | 2.981 | 3.010 | +4.6% | -6.1% | -14.0% |
| binario | 11464704 | 2.944 | 3.599 | 4.048 | 2.843 | 2.884 | 3.132 | +6.4% | -13.0% | -22.6% |
| cientifico | 21236957 | 2.519 | 2.956 | 3.363 | 2.653 | 2.700 | 3.813 | +51.3% | +29.0% | +13.4% |
| codigo | 8388605 | 5.876 | 8.171 | 8.912 | 5.759 | 5.880 | 6.970 | +18.6% | -14.7% | -21.8% |
| dna | 2319345 | 3.334 | 4.031 | 3.859 | 3.446 | 3.487 | 3.487 | +4.6% | -13.5% | -9.6% |
| sint_csv_vendas | 16777231 | 4.973 | 7.491 | 7.950 | 5.735 | 5.935 | 17.652 | +255.0% | +135.6% | +122.0% |
| sint_float32_telemetria | 16777216 | 4.382 | 5.733 | 7.030 | 4.788 | 4.896 | 10.769 | +145.8% | +87.8% | +53.2% |
| sint_incompressivel | 8388608 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | +0.0% | +0.0% | +0.0% |
| sint_int64_timestamps | 8388608 | 6.071 | 6.065 | 6.863 | 3.486 | 3.488 | 9.403 | +54.9% | +55.0% | +37.0% |
| sint_ja_comprimido | 4192940 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | +0.0% | +0.0% | +0.0% |
| sint_logs | 8388602 | 4.075 | 5.709 | 5.757 | 4.258 | 4.362 | 4.581 | +12.4% | -19.8% | -20.4% |
| sint_ndjson_eventos | 16777118 | 5.876 | 8.660 | 8.807 | 6.328 | 6.533 | 6.954 | +18.3% | -19.7% | -21.0% |
| texto | 20054724 | 3.226 | 4.228 | 4.281 | 3.455 | 3.560 | 3.588 | +11.2% | -15.1% | -16.2% |
| xml | 2050502 | 7.599 | 10.193 | 10.630 | 8.013 | 8.332 | 9.141 | +20.3% | -10.3% | -14.0% |

`OpenZL melhor` e o melhor entre generico, perfil e treinado, escolhido no proprio teste (otimista). Ganho = razao OpenZL / razao do baseline - 1.

## Por arquivo

| arquivo | classe | sintetico | bytes teste | zstd-1 | zstd-3 | zstd-19 | xz-9e | OZL gen. L6 | OZL gen. melhor L6/L9 | OZL perfil | OZL treinado | melhor OZL | vs zstd-3 | vs zstd-19 | vs xz-9e |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| dickens | texto | nao | 5096179 | 2.377 | 2.762 | 3.472 | 3.503 | 2.910 | 2.992 | - | 3.072 | treinado:serial | +11.2% | -11.5% | -12.3% |
| webster | texto | nao | 8388544 | 3.031 | 3.418 | 4.592 | 4.629 | 3.723 | 3.842 | - | - | generico:serial@L9 | +12.4% | -16.3% | -17.0% |
| reymont | texto | nao | 3309722 | 3.077 | 3.383 | 4.706 | 4.825 | 3.610 | 3.737 | - | - | generico:serial@L9 | +10.5% | -20.6% | -22.5% |
| bible.txt | texto | nao | 2023622 | 3.008 | 3.326 | 4.225 | 4.249 | 3.548 | 3.631 | - | - | generico:serial@L9 | +9.2% | -14.1% | -14.5% |
| world192.txt | texto | nao | 1236657 | 3.272 | 3.736 | 4.636 | 4.838 | 3.964 | 4.069 | - | - | generico:serial@L9 | +8.9% | -12.2% | -15.9% |
| samba | codigo | nao | 8388605 | 5.386 | 5.876 | 8.171 | 8.912 | 5.759 | 5.880 | - | 6.970 | treinado:serial | +18.6% | -14.7% | -21.8% |
| mozilla | binario | nao | 8388608 | 3.061 | 3.413 | 4.238 | 4.879 | 3.254 | 3.313 | - | 3.783 | treinado:serial | +10.8% | -10.7% | -22.5% |
| ooffice | binario | nao | 3076096 | 1.862 | 2.141 | 2.550 | 2.765 | 2.116 | 2.132 | - | - | generico:serial@L9 | -0.4% | -16.4% | -22.9% |
| sao | cientifico | nao | 3625972 | 1.148 | 1.305 | 1.444 | 1.628 | 1.364 | 1.375 | 2.011 | 3.072 | treinado:sao | +135.5% | +112.8% | +88.7% |
| nci | cientifico | nao | 8388583 | 12.341 | 12.322 | 20.480 | 22.499 | 12.879 | 13.238 | - | 15.727 | treinado:serial | +27.6% | -23.2% | -30.1% |
| x-ray | cientifico | nao | 4237120 | 1.233 | 1.364 | 1.628 | 1.881 | 1.451 | 1.490 | 1.644 | - | perfil:le-u16 | +20.6% | +1.0% | -12.6% |
| mr | cientifico | nao | 4985282 | 2.502 | 2.676 | 3.000 | 3.409 | 2.807 | 2.847 | 3.010 | 3.896 | treinado:le-u16 | +45.6% | +29.9% | +14.3% |
| osdb | banco_de_dados | nao | 5042844 | 2.701 | 2.878 | 3.206 | 3.501 | 2.940 | 2.981 | - | 3.010 | treinado:serial | +4.6% | -6.1% | -14.0% |
| xml | xml | nao | 2050502 | 6.747 | 7.599 | 10.193 | 10.630 | 8.013 | 8.332 | - | 9.141 | treinado:serial | +20.3% | -10.3% | -14.0% |
| E.coli | dna | nao | 2319345 | 3.292 | 3.334 | 4.031 | 3.859 | 3.446 | 3.487 | 3.433 | - | generico:serial@L9 | +4.6% | -13.5% | -9.6% |
| aleatorio.bin | sint_incompressivel | SIM | 8388608 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | - | - | generico:serial | +0.0% | +0.0% | +0.0% |
| logs.log | sint_logs | SIM | 8388602 | 4.095 | 4.075 | 5.709 | 5.757 | 4.258 | 4.362 | - | 4.581 | treinado:serial | +12.4% | -19.8% | -20.4% |
| ja_comprimido.xz | sint_ja_comprimido | SIM | 4192940 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | - | - | generico:serial | +0.0% | +0.0% | +0.0% |
| sint_vendas.csv | sint_csv_vendas | SIM | 16777231 | 4.684 | 4.973 | 7.491 | 7.950 | 5.735 | 5.935 | 6.569 | 17.652 | treinado:csv | +255.0% | +135.6% | +122.0% |
| sint_eventos.ndjson | sint_ndjson_eventos | SIM | 16777118 | 6.222 | 5.876 | 8.660 | 8.807 | 6.328 | 6.533 | - | 6.954 | treinado:serial | +18.3% | -19.7% | -21.0% |
| sint_telemetria_f32.bin | sint_float32_telemetria | SIM | 16777216 | 3.413 | 4.382 | 5.733 | 7.030 | 4.788 | 4.896 | 9.855 | 10.769 | treinado:le-u32 | +145.8% | +87.8% | +53.2% |
| sint_timestamps_i64.bin | sint_int64_timestamps | SIM | 8388608 | 6.071 | 6.071 | 6.065 | 6.863 | 3.486 | 3.488 | 9.330 | 9.403 | treinado:le-i64 | +54.9% | +55.0% | +37.0% |

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

## Referencias publicadas (apenas rotuladas; nao usadas para concluir)

- doc/mkdocs/doc/index.md do repositorio no commit fixado (tabela SAO, Silesia) | dado: sao | zstd-3=1.31, xz-9=1.64, openzl=2.06 | REFERENCIA PUBLICADA, nao usada para concluir; arquivo inteiro, perfil sao
- arXiv:2510.03203, tabela 5 (texto lido localmente) | dado: enwik7 (10^7 bytes de enwik) | zstd-3=2.8, zstd-19=3.58, xz-9=3.68, openzl=3.04 | REFERENCIA PUBLICADA, nao usada para concluir; o artigo diz que o compressor treinado usa zstd-6 em texto
- doc/mkdocs/doc/blog/posts/2026-09-24-compression-transformer.md (v0.3.0) | dado: 868 familias numericas, media geometrica ponderada | zstd-19=5.752, xz-9=5.92, openzl_nao_treinado=6.036, openzl_treinado=7.846, openzl_transformer_nivel7=7.76 | REFERENCIA PUBLICADA, nao usada para concluir; dados numericos reais de varias origens, nao os sinteticos daqui

## Tempo

NAO MEDIDO: fase B pendente, depende de janela exclusiva da maquina (`openzl_bench.py tempo`).
