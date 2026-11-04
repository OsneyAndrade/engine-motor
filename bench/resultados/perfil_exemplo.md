# Perfil de dataset

Gerado por `bench/perfil_dataset.py` (versao 1.0) em 2026-11-04 20:29:21. Somente leitura; semente 1. Saida ANONIMIZADA: nenhum nome de arquivo ou diretorio; so extensoes, classes e identificadores hash salgados (sal aleatorio descartado).

## Resumo executivo

1. Dataset: 204 arquivos, 285,02 MiB em 41 diretorios (mediana 223,90 KiB, p99 12,00 MiB); classes dominantes: texto 28,0%, binario 25,1%, cientifico_numerico 10,8%.
2. Fracao incompressivel: 11,5% por classificacao (midia, containers comprimidos, criptografado/aleatorio) e 11,5% [11,5%; 11,5%] por amostra (blocos com economia zstd-3 < 3%).
3. Dedup de arquivo inteiro: 20,3% dos bytes (57,77 MiB) sao copias exatas, em 36 grupos.
4. Familias de versoes: 55,3% dos bytes em 52 familias (27,7% sao versoes nao-base); o delta (patch-from) custa 11,1% [7,0%; 14,9%] do zstd-3 isolado em 52 pares medidos.
5. Compressao do dataset (blocos de 1 MiB, ponderada por bytes): zstd-3 economiza 52,9% [50,2%; 54,7%] (razao 2,12x); zstd-19 57,1%; xz-9e 59,5% [57,0%; 61,2%].
6. Ganho por classe (zstd-3, IC95): texto 70,3% [65,1%; 72,8%]; binario 50,8% [42,6%; 56,3%]; cientifico_numerico 16,3% [10,4%; 21,9%]; xml_html 78,0% [69,3%; 85,5%].
7. Economia mensal estimada so com compressao (politica por classe): US$ 3.678.229 [US$ 3.511.988; US$ 3.782.026] = armazenamento US$ 3.688.351 + movimento US$ 0,00 - CPU US$ 10.122.
8. Com dedup e delta: US$ 4.781.397 [US$ 4.655.560; US$ 4.830.217] por mes; recompressao assincrona recomendada em: texto->xz-9e, codigo->xz-9e, csv_tabular->xz-9e, json_ndjson->xz-9e, log->xz-9e, xml_html->xz-9e, binario->xz-9e, banco_de_dados->xz-9e, cientifico_numerico->xz-9e; stored em: imagem_audio_video, criptografado_aleatorio.
9. Parametros (premissas): volume 5,00 PB/mes, fracao retida 0,90, retencao 60 meses, armazenamento US$ 0,0230/GB-mes, nucleo US$ 0,0446/h, 3 leituras/GB, custo de movimento 0,00 (DESCONHECIDO).
10. Limites: amostra de blocos de 1 MiB e heuristicas de classe e familia; vazoes de outro corpus/CPU; desconhecido: o que o cliente chama de processamento e se o consumo aceita dado comprimido (ver secao de limites).

## 1. Inventario

| Item | Valor |
|---|---|
| Arquivos | 204 |
| Diretorios | 41 |
| Bytes | 285,02 MiB (298.860.417 B) |
| Inventario truncado | nao |
| Tamanho: media / mediana / p90 / p99 / p99,9 / max | 1,40 MiB / 223,90 KiB / 2,90 MiB / 12,00 MiB / 20,61 MiB / 20,61 MiB |
| Tamanho abaixo do qual ficam 50% / 90% / 99% dos bytes | 2,91 MiB / 20,61 MiB / 20,61 MiB |
| Extensoes distintas | 19 |


Extensoes por bytes (top 25)

| Extensao | Arquivos | Bytes | % dos bytes | Classe dominante |
|---|---|---|---|---|
| tar | 4 | 65,22 MiB | 22,9% | texto |
| txt | 28 | 50,46 MiB | 17,7% | texto |
| dat | 11 | 42,51 MiB | 14,9% | binario |
| bin | 12 | 20,00 MiB | 7,0% | criptografado_aleatorio |
| log | 7 | 17,41 MiB | 6,1% | log |
| raw | 5 | 13,47 MiB | 4,7% | cientifico_numerico |
| ndjson | 6 | 11,99 MiB | 4,2% | json_ndjson |
| dll | 2 | 11,74 MiB | 4,1% | binario |
| csv | 14 | 10,87 MiB | 3,8% | csv_tabular |
| (sem extensao) | 15 | 10,64 MiB | 3,7% | binario |
| xml | 2 | 10,20 MiB | 3,6% | xml_html |
| mp4 | 2 | 8,00 MiB | 2,8% | imagem_audio_video |
| sqlite | 2 | 4,11 MiB | 1,4% | banco_de_dados |
| py | 84 | 3,68 MiB | 1,3% | codigo |
| parquet | 2 | 2,00 MiB | 0,7% | container_comprimido |
| xz | 3 | 1,20 MiB | 0,4% | container_comprimido |
| jpg | 1 | 1,00 MiB | 0,4% | imagem_audio_video |
| gz | 2 | 265,33 KiB | 0,1% | container_comprimido |
| zip | 2 | 257,73 KiB | 0,1% | container_comprimido |


Entradas ignoradas e por que

| Motivo | Quantidade |
|---|---|
| link_simbolico | 4 |
| nao_regular | 1 |
| sem_permissao_diretorio | 1 |


## 2. Classificacao (matriz classe x bytes)

| Classe | Arquivos | Bytes | % dos bytes | Incompressivel por classificacao | Como foi classificado (arquivos) |
|---|---|---|---|---|---|
| texto | 25 | 79,68 MiB | 28,0% | nao | assinatura=2, conteudo=23 |
| binario | 23 | 71,43 MiB | 25,1% | nao | assinatura=19, conteudo_binario=4 |
| cientifico_numerico | 11 | 30,87 MiB | 10,8% | nao | entropia_dos_deltas=6, extensao=5 |
| xml_html | 6 | 22,20 MiB | 7,8% | nao | assinatura=2, conteudo=4 |
| criptografado_aleatorio | 12 | 20,00 MiB | 7,0% | sim | entropia>=7.9_sem_assinatura=12 |
| log | 7 | 17,41 MiB | 6,1% | nao | extensao=7 |
| json_ndjson | 6 | 11,99 MiB | 4,2% | nao | conteudo_ndjson=6 |
| csv_tabular | 14 | 10,87 MiB | 3,8% | nao | extensao=14 |
| imagem_audio_video | 3 | 9,00 MiB | 3,2% | sim | assinatura=3 |
| banco_de_dados | 2 | 4,11 MiB | 1,4% | nao | assinatura=2 |
| container_comprimido | 9 | 3,71 MiB | 1,3% | sim | assinatura=9 |
| codigo | 84 | 3,68 MiB | 1,3% | nao | extensao=84 |
| ilegivel | 1 | 64,00 KiB | 0,0% | nao | - |
| vazio | 1 | 0 B | 0,0% | nao | tamanho_zero=1 |


Fracao incompressivel: **11,5%** por classificacao; **11,5%** [11,5%; 11,5%] por amostra (blocos de 1 MiB com economia zstd-3 menor que 3%, ponderada por bytes).


## 3. Dedup de arquivo inteiro

Modo: pre_filtro_tamanho+hash_de_amostra, hash_completo_so_nos_candidatos. Candidatos por tamanho: 90 arquivos (136,62 MiB); apos hash de amostra: 79; bytes lidos no hash de amostra: 12,77 MiB; no hash completo: 109,15 MiB.

Grupos de duplicatas: 36; arquivos redundantes: 41; bytes duplicados: 57,77 MiB (20,3% do dataset).

| Classe | Bytes da classe | Bytes duplicados | % da classe |
|---|---|---|---|
| texto | 79,68 MiB | 9,70 MiB | 12,2% |
| codigo | 3,68 MiB | 708,58 KiB | 18,8% |
| csv_tabular | 10,87 MiB | 1,88 MiB | 17,3% |
| json_ndjson | 11,99 MiB | 2,00 MiB | 16,7% |
| log | 17,41 MiB | 0 B | 0,0% |
| xml_html | 22,20 MiB | 8,10 MiB | 36,5% |
| binario | 71,43 MiB | 13,18 MiB | 18,4% |
| banco_de_dados | 4,11 MiB | 0 B | 0,0% |
| cientifico_numerico | 30,87 MiB | 8,49 MiB | 27,5% |
| imagem_audio_video | 9,00 MiB | 4,00 MiB | 44,4% |
| container_comprimido | 3,71 MiB | 1,73 MiB | 46,6% |
| criptografado_aleatorio | 20,00 MiB | 8,00 MiB | 40,0% |


Maiores grupos de duplicatas (identificador hash salgado)

| Id | Copias | Tamanho | Bytes redundantes | Classe |
|---|---|---|---|---|
| fae2efa5165a7d19 | 2 | 12,00 MiB | 12,00 MiB | binario |
| 5e8e5653a280006c | 4 | 2,00 MiB | 6,00 MiB | criptografado_aleatorio |
| f20b7ad731e1e630 | 2 | 5,10 MiB | 5,10 MiB | xml_html |
| fc48e0fdcced7a3b | 3 | 2,38 MiB | 4,77 MiB | texto |
| b39bc6e502d9607b | 2 | 4,00 MiB | 4,00 MiB | imagem_audio_video |
| c7ae191a85899692 | 2 | 3,00 MiB | 3,00 MiB | xml_html |
| 0e3c7a76dc8dd7b1 | 2 | 2,90 MiB | 2,90 MiB | cientifico_numerico |
| 02bbc92f731c0b63 | 2 | 2,90 MiB | 2,90 MiB | cientifico_numerico |
| 3789e116b3d06260 | 2 | 2,69 MiB | 2,69 MiB | cientifico_numerico |
| 2245f6dd7670434f | 2 | 2,38 MiB | 2,38 MiB | texto |


## 4. Familias de versoes e delta

Criterio: tres passadas, cada arquivo entra na primeira que o agrupa: (A) diretorio-pai normalizado (digitos viram #) + mesmo nome; (B) mesmo diretorio + radical do nome sem datas, versoes e marcas de copia; (C) diretorio-pai normalizado + radical normalizado + mesma extensao; membros ordenados pelo caminho (ordem natural), com tamanhos consecutivos dentro de x1.5.

Familias: 52 com 104 membros; bytes em familias: 157,62 MiB (55,3%); versoes nao-base: 79,02 MiB (27,7%). Pares possiveis: 52; medidos: 52 (janela de 16 MiB por arquivo); pares com falha de roundtrip: 0.

| Classe | % bytes em familias | % bytes nao-base | Pares medidos | patch-from / zstd-3 isolado | IC95 | Baixa confianca |
|---|---|---|---|---|---|---|
| texto | 74,4% | 37,2% | 6 | 16,1% | [11,7%; 18,1%] | nao |
| codigo | 56,8% | 30,2% | 26 | 21,8% | [19,0%; 24,9%] | nao |
| csv_tabular | 65,4% | 32,8% | 4 | 11,0% | [10,7%; 11,5%] | sim |
| json_ndjson | 66,7% | 33,4% | 2 | 11,3% | [11,3%; 11,3%] | sim |
| log | 85,7% | 42,9% | 3 | 10,6% | [10,0%; 11,0%] | sim |
| xml_html | 27,0% | 13,5% | 1 | 12,6% | [12,6%; 12,6%] | sim |
| binario | 62,9% | 35,5% | 8 | 8,4% | [2,6%; 16,1%] | nao |
| banco_de_dados | 100,0% | 51,9% | 1 | 11,8% | [11,8%; 11,8%] | sim |
| cientifico_numerico | 36,3% | 8,7% | 1 | 8,8% | [8,8%; 8,8%] | sim |
| imagem_audio_video | 0,0% | 0,0% | 0 | - | - | - |
| container_comprimido | 0,0% | 0,0% | 0 | - | - | - |
| criptografado_aleatorio | 0,0% | 0,0% | 0 | - | - | - |


Maiores familias

| Id | Membros | Bytes | Classe | patch/z3 medido |
|---|---|---|---|---|
| 3af40998d167e602 | 2 | 41,22 MiB | texto | 18,2% |
| 7fe0f30e635fb8b6 | 2 | 19,24 MiB | binario | 18,8% |
| 2a1e0c17c9857fd7 | 2 | 11,74 MiB | binario | 0,2% |
| 4782ba59d02cce1d | 2 | 7,72 MiB | texto | 12,9% |
| da1f7aa1439ecdd5 | 2 | 6,00 MiB | xml_html | 12,6% |
| adf47866bce26098 | 2 | 5,81 MiB | binario | 4,8% |
| 89dceca341b1cd8d | 2 | 5,80 MiB | binario | 4,8% |
| 6fa8783b0e3b852d | 2 | 5,39 MiB | cientifico_numerico | 8,8% |
| 1e7b8f1b1fe62cb0 | 2 | 4,98 MiB | log | 11,0% |
| c824758d2c63f93e | 2 | 4,98 MiB | log | 10,0% |


## 5. Compressibilidade por classe (blocos independentes de 1 MiB)

Estimador: razao de somas ponderada por bytes; IC95 por bootstrap (bootstrap de Poisson por arquivo (por bloco quando ha menos de 5 arquivos amostrados), 1000 reamostras, mesmos pesos para todos os codecs da classe). Cada celula: economia (razao x). Codecs rapidos em ate 256 MiB por classe; lentos em ate 32 MiB. 'cru%' = blocos armazenados sem compressao.

| Classe | Arquivos | % amostrado | zstd-1 | zstd-3 [IC95] | zstd-9 | zstd-19 | xz-9e [IC95] | bzip2-9 | cru% | Confianca |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 25 | 100,0% | 67,1% (3,04x) | 70,3% (3,37x) [65,1%; 72,8%] | 72,8% (3,68x) | 75,2% (4,04x) | 75,8% (4,13x) [71,0%; 78,0%] | 76,0% (4,17x) | 0,0% | ok |
| codigo | 84 | 100,0% | 73,4% (3,76x) | 74,7% (3,95x) [73,8%; 75,4%] | 77,1% (4,37x) | 78,3% (4,61x) | 78,5% (4,65x) [77,5%; 79,3%] | 78,9% (4,74x) | 0,0% | ok |
| csv_tabular | 14 | 100,0% | 69,3% (3,26x) | 66,5% (2,98x) [66,4%; 66,6%] | 70,5% (3,38x) | 73,8% (3,82x) | 79,7% (4,94x) [79,7%; 79,8%] | 77,2% (4,39x) | 0,0% | ok |
| json_ndjson | 6 | 100,0% | 85,8% (7,05x) | 85,7% (7,00x) [85,7%; 85,7%] | 87,9% (8,26x) | 89,7% (9,71x) | 90,7% (10,77x) [90,7%; 90,7%] | 91,1% (11,21x) | 0,0% | ok |
| log | 7 | 100,0% | 78,9% (4,74x) | 78,6% (4,67x) [78,5%; 78,7%] | 81,9% (5,51x) | 83,5% (6,06x) | 84,3% (6,35x) [84,2%; 84,3%] | 86,2% (7,24x) | 0,0% | ok |
| xml_html | 6 | 100,0% | 75,7% (4,12x) | 78,0% (4,55x) [69,3%; 85,5%] | 80,8% (5,22x) | 82,5% (5,72x) | 82,8% (5,83x) [75,3%; 89,3%] | 84,5% (6,43x) | 0,0% | ok |
| binario | 23 | 100,0% | 47,1% (1,89x) | 50,8% (2,03x) [42,6%; 56,3%] | 52,4% (2,10x) | 54,8% (2,21x) | 57,7% (2,37x) [50,7%; 62,9%] | 53,5% (2,15x) | 0,0% | ok |
| banco_de_dados | 2 | 100,0% | 51,6% (2,07x) | 55,6% (2,25x) [54,8%; 58,4%] | 56,8% (2,31x) | 59,9% (2,50x) | 62,5% (2,66x) [61,9%; 64,7%] | 67,6% (3,09x) | 0,0% | BAIXA |
| cientifico_numerico | 11 | 100,0% | 12,6% (1,14x) | 16,3% (1,19x) [10,4%; 21,9%] | 18,3% (1,22x) | 21,5% (1,27x) | 32,0% (1,47x) [24,1%; 39,3%] | 25,3% (1,34x) | 0,0% | ok |
| imagem_audio_video | 3 | 100,0% | 0,0% (1,00x) | 0,0% (1,00x) [0,0%; 0,0%] | 0,0% (1,00x) | 0,0% (1,00x) | 0,0% (1,00x) [0,0%; 0,0%] | 0,0% (1,00x) | 100,0% | BAIXA |
| container_comprimido | 9 | 100,0% | 0,0% (1,00x) | 0,0% (1,00x) [0,0%; 0,0%] | 0,0% (1,00x) | 0,0% (1,00x) | 0,0% (1,00x) [-0,0%; 0,0%] | 0,0% (1,00x) | 81,8% | ok |
| criptografado_aleatorio | 12 | 100,0% | 0,0% (1,00x) | 0,0% (1,00x) [0,0%; 0,0%] | 0,0% (1,00x) | 0,0% (1,00x) | 0,0% (1,00x) [0,0%; 0,0%] | 0,0% (1,00x) | 100,0% | ok |


Dataset inteiro (ponderado por bytes das classes amostradas): zstd-1 50,6% [47,4%; 52,5%] (2,03x); zstd-3 52,9% [50,2%; 54,7%] (2,12x); zstd-9 55,0% [52,2%; 56,8%] (2,22x); zstd-19 57,1% [54,4%; 58,9%] (2,33x); xz-9e 59,5% [57,0%; 61,2%] (2,47x); bzip2-9 58,0% [54,9%; 60,4%] (2,38x).


Transform por bloco em `cientifico_numerico` (melhor entre zstd-3 puro e {delta 1/2/4/8, byte_split 4/8}+zstd-3 por bloco de 1 MiB (oraculo: o planejador real nao conhece o resultado antes de comprimir; limite superior)): melhor por bloco economiza 31,6% (razao 1,46x), ganho de 15,29 pp sobre zstd-3 [14,49; 16,17]; escolha por bloco: d2=9%, b4=36%, b8=55%.


Vazao de compressao medida nesta execucao (indicativa, vazao_comp_indicativa_MBps foi medida com ate 5 processos concorrentes, sem fixar CPU, na maquina atual: so indicativa; a estimativa economica usa bench/resultados/medicoes.json (rotulada)): texto zstd-1=517/zstd-3=376/zstd-9=91/zstd-19=6/xz-9e=4/bzip2-9=24; codigo zstd-1=434/zstd-3=377/zstd-9=84/zstd-19=5/xz-9e=7/bzip2-9=24; csv_tabular zstd-1=462/zstd-3=313/zstd-9=72/zstd-19=5/xz-9e=5/bzip2-9=26 MB/s (primeiras classes).


## 6. Estimativa economica

Formula: armazenamento = s_arm x fracao_retida x volume_GB x retencao x preco_GB_mes; movimento = s_compressao x volume_GB x custo_movimento x passagens; CPU = volume_GB x (custo_comp_GB + leituras x custo_decomp_GB), custo_GB = preco_nucleo_h / (vazao_MBps x eficiencia x 3,6); identica a docs/anexos/modelo_cliente.py (conferida em autoteste). Acrescimos deste perfil: dedup e delta reduzem s_arm; recompressao assincrona segue a regra de M* de resultados/resumo.md secao 11.

### Parametros (valor, fonte e tipo)

| Parametro | Valor | Unidade | Tipo | Fonte |
|---|---|---|---|---|
| volume_mensal_bytes | 5.000.000.000.000.000 | B/mes | premissa | requisito do pedido: 5 PB/mes de processamento (PB = 10^15 B); bench/precos.json |
| fracao_retida | 0,9000 | fracao do volume processado | premissa | premissa: o cliente armazena 'um pouco menos' do que processa (docs/pesquisa-compressao-neural.md 9.1); trocar pelo numero real |
| retencao_meses | 60,0000 | meses | premissa | premissa do pedido anterior (T = 60 meses, bench/precos.json e docs/anexos/modelo_cliente.py) |
| preco_armazenamento_gb_mes | 0,0230 | USD/GB-mes | verificado | AWS S3 Standard, tabela publica (bench/precos.json); o preco real do cliente e desconhecido |
| preco_nucleo_hora | 0,0446 | USD/vCPU-h | verificado | AWS c7i, Vantage 2026-11-03 (bench/precos.json); o custo de CPU real do cliente e desconhecido |
| custo_movimento_usd_por_gb | 0,0000 | USD/GB movido | desconhecido | DESCONHECIDO: usado 0 (mesma regiao) ate o cliente informar; ver sensibilidade (referencia: egress S3 US$0,09/GB) |
| passagens_movimento | 1,0000 | passagens por mes | desconhecido | DESCONHECIDO: quantas vezes o dado atravessa o transporte por mes |
| leituras_por_gb_processado | 3,0000 | leituras (descompressoes) por GB processado | premissa | premissa: modelo_cliente.py usa 1, 3 e 10; o volume lido real e desconhecido |
| eficiencia_escala | 1,0000 | fator multiplicativo sobre a vazao por nucleo | premissa | premissa do modelo_cliente.py (1,0, otimista: medido 0,42 a 0,95 em CPU hibrida) |
| idade_recompressao_meses | 1,0000 | meses | premissa | premissa: recompressao assincrona 1 mes depois da ingestao (dado ja em zstd-3) |
| vazao_hash_mbps | 1.160 | MB/s por nucleo | medido | medido em outro corpus: blake2b em 1 P-core (docs/pesquisa-compressao-neural.md 10.8, 1,16 a 1,25 GB/s) |
| dedup_aplicavel | sim | booleano | premissa | premissa: o produto elimina arquivos inteiros duplicados (gate F1: dedup de arquivo inteiro ganha das alternativas em arvores) |
| delta_aplicavel | sim | booleano | premissa | premissa: o produto usa referencia entre versoes (zstd --patch-from, janela longa) em familias de arquivos |
| consumo_aceita_dado_comprimido | (sem valor) | booleano | desconhecido | DESCONHECIDO: o que o cliente chama de 'processamento' e se o consumo aceita dado comprimido; sem isso o ganho de movimento nao e garantido |
| vazao_mbps | (sem valor) | MB/s por nucleo por classe e codec | medido | opcional: {classe_do_perfil: {codec: {"comp": x, "decomp": y}}}; padrao = bench/resultados/medicoes.json (bloco de 1 MiB, outro corpus e outra CPU) |


### Por classe (politica recomendada, com dedup e delta quando aplicaveis)

| Classe | % bytes | Economia zstd-3 | Dedup | Economia de armazenamento total | Armazenamento/mes | Movimento/mes | CPU/mes | Liquido/mes | IC95 liquido | Politica |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 28,0% | 70,3% | 12,2% | 83,2% | US$ 1.497.641 | US$ 0,00 | US$ 3.135 | US$ 1.494.506 | [US$ 1.446.518; US$ 1.518.460] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=3,1 meses) |
| codigo | 1,3% | 74,7% | 18,8% | 85,4% | US$ 70.318 | US$ 0,00 | US$ 111 | US$ 70.208 | [US$ 69.728; US$ 70.612] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=3,2 meses) |
| csv_tabular | 3,8% | 66,5% | 17,3% | 82,1% | US$ 210.897 | US$ 0,00 | US$ 405 | US$ 210.491 | [US$ 210.383; US$ 210.568] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=1,3 meses) |
| json_ndjson | 4,2% | 85,7% | 16,7% | 92,3% | US$ 248.203 | US$ 0,00 | US$ 367 | US$ 247.836 | [US$ 240.558; US$ 247.864] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=2,9 meses) |
| log | 6,1% | 78,6% | 0,0% | 86,8% | US$ 342.439 | US$ 0,00 | US$ 728 | US$ 341.711 | [US$ 318.637; US$ 341.867] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=3,0 meses) |
| xml_html | 7,8% | 78,0% | 36,5% | 88,6% | US$ 440.678 | US$ 0,00 | US$ 527 | US$ 440.151 | [US$ 408.124; US$ 455.225] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=2,4 meses) |
| binario | 25,1% | 50,8% | 18,4% | 75,9% | US$ 1.233.175 | US$ 0,00 | US$ 2.158 | US$ 1.231.017 | [US$ 1.173.340; US$ 1.274.105] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=1,8 meses) |
| banco_de_dados | 1,4% | 55,6% | 0,0% | 75,9% | US$ 71.276 | US$ 0,00 | US$ 147 | US$ 71.129 | [US$ 55.245; US$ 71.761] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=2,3 meses) |
| cientifico_numerico | 10,8% | 16,3% | 27,5% | 46,0% | US$ 376.254 | US$ 0,00 | US$ 1.105 | US$ 375.149 | [US$ 309.683; US$ 405.360] | zstd-3 sincrono + recompressao assincrona para xz-9e (M*=0,8 meses) |
| imagem_audio_video | 3,2% | 0,0% | 44,4% | 44,4% | US$ 87.173 | US$ 0,00 | US$ 1,69 | US$ 87.171 | [US$ 87.171; US$ 87.171] | stored (sem compressao; zstd-3 nao paga o CPU nesta classe) |
| container_comprimido | 1,3% | 0,0% | 46,6% | 46,6% | US$ 37.688 | US$ 0,00 | US$ 1,16 | US$ 37.687 | [US$ 37.686; US$ 37.691] | zstd-3 sincrono (recompressao assincrona nao paga na vida restante) |
| criptografado_aleatorio | 7,0% | 0,0% | 40,0% | 40,0% | US$ 174.345 | US$ 0,00 | US$ 3,75 | US$ 174.341 | [US$ 174.341; US$ 174.341] | stored (sem compressao; zstd-3 nao paga o CPU nesta classe) |
| **TOTAL** | 100% |  |  |  | US$ 4.790.086 | US$ 0,00 | US$ 8.689 | US$ 4.781.397 | [US$ 4.655.560; US$ 4.830.217] |  |


Valores em US$ por mes em regime estavel. Armazenamento = economia acumulada nos meses de retencao; liquido = armazenamento + movimento - CPU (compressao, leituras, hash do dedup, delta e recompressao).

Apenas compressao (sem dedup nem delta): liquido US$ 3.678.229 [US$ 3.511.988; US$ 3.782.026].


### Recompressao assincrona (regra de M* de resultados/resumo.md, secao 11)

| Classe | Destino | Delta economia (pp) | Custo de recompressao (US$/GB) | M* (meses) | Vida restante (meses) | Nucleos medios | Paga na vida restante |
|---|---|---|---|---|---|---|---|
| texto | zstd-19 | 4,96 | 0,0019 | 2,9 | 59 | 74,0 | sim |
| texto | xz-9e | 5,51 | 0,0022 | 3,1 | 59 | 86,6 | sim |
| codigo | zstd-19 | 3,63 | 0,0012 | 2,5 | 59 | 2,2 | sim |
| codigo | xz-9e | 3,81 | 0,0016 | 3,2 | 59 | 2,9 | sim |
| csv_tabular | zstd-19 | 7,32 | 0,0018 | 2,0 | 59 | 9,5 | sim |
| csv_tabular | xz-9e | 13,25 | 0,0021 | 1,3 | 59 | 11,1 | sim |
| json_ndjson | zstd-19 | 3,98 | 0,0019 | 3,9 | 59 | 11,5 | sim |
| json_ndjson | xz-9e | 4,99 | 0,0018 | 2,9 | 59 | 10,7 | sim |
| log | zstd-19 | 4,93 | 0,0020 | 2,9 | 59 | 17,4 | sim |
| log | xz-9e | 5,68 | 0,0024 | 3,0 | 59 | 20,3 | sim |
| xml_html | zstd-19 | 4,50 | 0,0015 | 2,8 | 59 | 16,2 | sim |
| xml_html | xz-9e | 4,83 | 0,0014 | 2,4 | 59 | 15,1 | sim |
| binario | zstd-19 | 4,06 | 0,0010 | 2,2 | 59 | 34,7 | sim |
| binario | xz-9e | 6,97 | 0,0014 | 1,8 | 59 | 50,3 | sim |
| banco_de_dados | zstd-19 | 4,32 | 0,0016 | 2,9 | 59 | 3,1 | sim |
| banco_de_dados | xz-9e | 6,84 | 0,0019 | 2,3 | 59 | 3,9 | sim |
| cientifico_numerico | zstd-19 | 5,26 | 0,0012 | 1,5 | 59 | 18,0 | sim |
| cientifico_numerico | xz-9e | 15,67 | 0,0018 | 0,8 | 59 | 27,7 | sim |
| container_comprimido | zstd-19 | 0,00 | 0,0002 | 2.844 | 59 | 0,3 | nao |
| container_comprimido | xz-9e | -0,00 | 0,0006 | nunca | 59 | 1,1 | nao |


### Sensibilidade (liquido total em US$/mes)

| Fracao retida | Cenario de armazenamento | US$/GB-mes | Liquido/mes |
|---|---|---|---|
| 0,80 | s3_standard | 0,02300 | US$ 4.249.981 |
| 0,80 | b2 | 0,00695 | US$ 1.278.751 |
| 0,80 | glacier_ir | 0,00400 | US$ 732.644 |
| 0,80 | onprem | 0,00200 | US$ 367.869 |
| 0,90 | s3_standard | 0,02300 | US$ 4.781.397 |
| 0,90 | b2 | 0,00695 | US$ 1.438.762 |
| 0,90 | glacier_ir | 0,00400 | US$ 824.391 |
| 0,90 | onprem | 0,00200 | US$ 413.903 |
| 0,95 | s3_standard | 0,02300 | US$ 5.047.105 |
| 0,95 | b2 | 0,00695 | US$ 1.518.767 |
| 0,95 | glacier_ir | 0,00400 | US$ 870.265 |
| 0,95 | onprem | 0,00200 | US$ 436.920 |


| Custo de movimento (US$/GB) | Passagens | Movimento/mes | Liquido/mes |
|---|---|---|---|
| 0,00 | 1 | US$ 0,00 | US$ 4.781.397 |
| 0,00 | 3 | US$ 0,00 | US$ 4.781.397 |
| 0,01 | 1 | US$ 26.465 | US$ 4.807.862 |
| 0,01 | 3 | US$ 79.396 | US$ 4.860.793 |
| 0,05 | 1 | US$ 132.327 | US$ 4.913.724 |
| 0,05 | 3 | US$ 396.980 | US$ 5.178.377 |
| 0,09 | 1 | US$ 238.188 | US$ 5.019.585 |
| 0,09 | 3 | US$ 714.564 | US$ 5.495.961 |


| Leituras por GB processado | CPU/mes | Liquido/mes |
|---|---|---|
| 1 | US$ 7.947 | US$ 4.782.139 |
| 3 | US$ 8.689 | US$ 4.781.397 |
| 10 | US$ 11.286 | US$ 4.778.799 |


## 7. O que e medido no dataset, o que e premissa e o que e desconhecido

**Medido no dataset**

- numero de arquivos, bytes, percentis de tamanho, extensoes (inventario completo da raiz, so leitura de metadados)
- classe de cada arquivo (heuristica de extensao + bytes magicos + entropia de amostra de ate 48 KiB; nao e verdade absoluta)
- bytes de arquivos inteiros duplicados (hash de amostra e hash completo dos candidatos)
- familias de versoes (heuristica de nome/diretorio/tamanho) e a razao patch/zstd-3 em ate 200 pares (janela de 16 MiB por arquivo)
- razao de compressao por classe (zstd-1/3/9/19, xz-9e, bzip2-9) em blocos de 1 MiB sorteados, com IC95 por bootstrap
- fracao incompressivel por classificacao e por amostra (blocos com economia zstd-3 < 3%)

**Medido em outro corpus**

- vazao por nucleo de cada codec por classe: bench/resultados/medicoes.json (Silesia, Canterbury e sinteticos, i9-14900HX, tmpfs); a classe do cliente usa a classe medida mais parecida (proxy rotulado)
- vazao do hash (blake2b) do dedup: bench/resultados/dedup.md

**Premissa**

- volume mensal de 5,00 PB e fracao retida 0,90 (o dataset e tratado como representativo do fluxo mensal)
- precos (S3 Standard, c7i), retencao de 60 meses, eficiencia de escala 1,00, leituras por GB 3
- dado duplicado e dado de familia tem a mesma compressibilidade da media da classe
- leituras uniformes no tempo; recompressao assincrona 1 mes(es) apos a ingestao
- o produto realiza dedup de arquivo inteiro e delta por familia (ver dedup_aplicavel e delta_aplicavel)
- o ganho de transform (delta/byte_split) e oraculo por bloco: limite superior, nao entra na estimativa economica

**Desconhecido**

- o que o cliente chama de 'processamento' e se o consumo aceita dado comprimido (define se ha ganho de movimento e de CPU do cliente)
- custo real de movimento de dados e numero de passagens (usado US$ 0,00/GB e 1 passagem(ns))
- volume realmente lido, padrao quente/frio e popularidade por objeto
- precos reais de armazenamento e computacao do cliente, descontos, custo de recuperacao e de saida
- se o dado ja chega comprimido ou criptografado na origem do fluxo (alem do que existe neste dataset)
- SLA de latencia de leitura, restricoes de LGPD e de residencia do dado
- se a amostra recebida representa o fluxo de 5 PB/mes (sazonalidade, novos tipos de dado)

Vazoes usadas por classe (proxy)

| Classe | zstd-3 comp MB/s | zstd-3 decomp MB/s | Origem |
|---|---|---|---|
| texto | 325 | 1.632 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'texto' usada como proxy de 'texto' |
| codigo | 517 | 2.270 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'codigo' usada como proxy de 'codigo' |
| csv_tabular | 325 | 1.632 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'texto' usada como proxy de 'csv_tabular' |
| json_ndjson | 805 | 2.402 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'xml' usada como proxy de 'json_ndjson' |
| log | 446 | 1.764 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'sint_logs' usada como proxy de 'log'; proxy SINTETICO |
| xml_html | 805 | 2.402 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'xml' usada como proxy de 'xml_html' |
| binario | 411 | 1.524 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'binario' usada como proxy de 'binario' |
| banco_de_dados | 453 | 1.817 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'banco_de_dados' usada como proxy de 'banco_de_dados' |
| cientifico_numerico | 348 | 1.412 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'cientifico' usada como proxy de 'cientifico_numerico' |
| imagem_audio_video | 5.126 | 7.929 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'sint_ja_comprimido' usada como proxy de 'imagem_audio_video'; proxy SINTETICO |
| container_comprimido | 5.126 | 7.929 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'sint_ja_comprimido' usada como proxy de 'container_comprimido'; proxy SINTETICO |
| criptografado_aleatorio | 5.539 | 8.369 | medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe 'sint_incompressivel' usada como proxy de 'criptografado_aleatorio'; proxy SINTETICO |


## 8. Registro de leitura

| Item | Valor |
|---|---|
| Arquivos inventariados | 204 |
| Arquivos com leitura | 202 |
| Bytes lidos (total) | 560,71 MiB (196,73% do inventario) |
| Bytes lidos por fase | classificacao=5,44 MiB, dedup=121,92 MiB, amostra_compressibilidade=284,95 MiB, delta=148,40 MiB |
| Ilegiveis | 1 {'sem_permissao_leitura': 1} |
| Ignorados por motivo | {'nao_regular': 1, 'link_simbolico': 4, 'sem_permissao_diretorio': 1} |
| Diretorios inacessiveis | 1 |
| Nomes com bytes invalidos em UTF-8 (processados normalmente) | 2 |
| Alterados durante a leitura | 0 |
| Abertura | O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK, tentando O_NOATIME (negado em arquivos de outro dono: reabre sem) |
| Tempo por fase (s) | {'inventario': 0.001, 'classificacao': 0.046, 'dedup': 0.064, 'familias': 0.001, 'delta': 0.12, 'amostragem': 15.904} |


O registro por arquivo esta em `registro_leitura_<etapa>.jsonl` (um objeto por linha: identificador, extensao, classe, tamanho, bytes lidos por fase, status).


## 9. Limites e o que nao sabemos

- A classificacao e uma heuristica (extensao, bytes magicos e entropia de uma amostra de ate 48 KiB por arquivo); arquivos mal classificados mudam a vazao e a politica atribuidas, nao a razao medida (que e calculada sobre os blocos reais da classe atribuida).
- A razao por classe vem de blocos de 1 MiB sorteados (ou de todos os blocos quando a classe cabe no orcamento); a amostra pode perder estruturas raras e nao ve redundancia entre blocos (cada bloco e comprimido sozinho, como no motor).
- O IC95 e de bootstrap por arquivo: cobre a variacao entre arquivos da amostra, nao a diferenca entre este dataset e o fluxo real de 5 PB/mes.
- Classes com menos de 5 arquivos estao marcadas como baixa confianca: o IC por bloco subestima a incerteza entre arquivos.
- Dedup cobre so arquivos inteiros identicos; nao ha dedup por bloco nem por similaridade alem das familias; o hash de amostra so serve de pre-filtro, a confirmacao e pelo hash completo.
- Familias de versoes sao inferidas por nome, diretorio e tamanho; a razao patch/zstd-3 e medida em ate 200 pares por janela de 16 MiB (prefixo ou janela alinhada) e extrapolada a todos os pares da classe; pares fora da amostra podem ser mais ou menos parecidos.
- O ganho de transform (delta/byte_split) e oraculo por bloco, um limite superior; a classe cientifico_numerico so aparece quando a entropia dos deltas cai pelo menos 0.75 bit.
- As vazoes por nucleo vem de bench/resultados/medicoes.json (outro corpus, CPU de notebook hibrido, tmpfs, sem I/O); classes sem medida propria usam proxy rotulado (log, ja comprimido e aleatorio usam classes SINTETICAS).
- O custo de CPU nao inclui I/O, rede, metadados, erasure coding, migracao do dado existente nem o custo de operar o indice de dedup a 5 PB/mes.
- Nao ha modelo de popularidade por objeto: as leituras sao uniformes no tempo.
- O volume mensal e a fracao retida sao premissas; o perfil do dataset recebido e extrapolado ao fluxo mensal sem correcao de sazonalidade.
- Desconhecido (perguntar ao cliente): o que ele chama de 'processamento' e se o consumo aceita dado comprimido; custo de movimento e passagens; volume lido e padrao quente/frio; precos reais; se o dado ja chega comprimido ou criptografado; SLA de latencia; restricoes de LGPD.
- Fracao incompressivel por classificacao conta so classes ja comprimidas, midia e criptografado; a por amostra conta qualquer bloco com economia zstd-3 menor que 3% e e a mais confiavel.
- Entradas ignoradas (nao medidas): link_simbolico=4, nao_regular=1, sem_permissao_diretorio=1.
- Arquivos ilegiveis (contados no inventario, sem classe nem estimativa): sem_permissao_leitura=1.
- Classes de baixa confianca (poucos arquivos): banco_de_dados, imagem_audio_video.
- Bytes sem estimativa economica (ilegiveis ou sem amostra): 64,00 KiB.
