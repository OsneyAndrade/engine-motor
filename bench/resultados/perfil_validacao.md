# Validacao do perfil de dataset

Gerado por `bench/perfil_validar.py` em 2026-11-04 20:17:50 (Python 3.14.4, zstd 1.5.7). Corpora publicos regenerados com `preparar_dados.py` (Silesia, Canterbury Large e sinteticos) e um dataset versionado sintetico. Nada deste arquivo vem de dado de cliente; nomes de arquivo aparecem porque os dados sao publicos ou sinteticos.

## Resultado do autoteste

autoteste: 41 testes, 41 passaram, 0 falharam, 43s

## A. Corpora publicos: razao por amostra contra as medidas de `medicoes.json`

Tres referencias: (1) **censo**: a mesma ferramenta com todos os blocos de 1 MiB do arquivo inteiro (isola o erro de amostragem); (2) **medicoes, bloco de 1 MiB**: `principal` com `modo=bloco_1MiB`, so os primeiros 8 MiB de cada arquivo (iguala exatamente o censo nos arquivos de ate 8 MiB); (3) **medicoes, inteira**: `inteiro` (quadro unico) nos arquivos de ate 8 MiB e `completo` (arquivo todo, quadro unico) para zstd-1/3/9 nos maiores. Erro = (estimativa - referencia) / referencia da razao, em %.

| Codec | orcamento padrao vs censo (medio / max %, n) | censo vs medicoes bloco 1 MiB, arquivos <= 8 MiB (medio / max %, n) | orcamento padrao vs medicoes inteira (medio / max %, n) | censo vs medicoes inteira (medio / max %, n) |
|---|---|---|---|---|
| zstd-1 | 0,000 / 0,000 / 18 | 0,000 / 0,003 / 8 | 0,46 / 1,12 / 18 | 0,46 / 1,12 / 18 |
| zstd-3 | 0,000 / 0,000 / 18 | 0,000 / 0,003 / 8 | 1,43 / 4,57 / 18 | 1,43 / 4,57 / 18 |
| zstd-9 | 0,000 / 0,000 / 18 | 0,000 / 0,003 / 8 | 3,37 / 7,75 / 18 | 3,37 / 7,75 / 18 |
| zstd-19 | 1,732 / 7,656 / 18 | 0,000 / 0,003 / 8 | 4,38 / 8,64 / 8 | 4,44 / 9,57 / 8 |
| xz-9e | 1,809 / 7,709 / 18 | 0,001 / 0,011 / 8 | 4,78 / 9,27 / 8 | 4,73 / 9,64 / 8 |
| bzip2-9 | 1,682 / 6,679 / 18 | 0,059 / 0,473 / 8 | 1,62 / 5,08 / 8 | 1,11 / 3,99 / 8 |


O orcamento padrao (256 MiB por classe para zstd-1/3/9) e censo em todo o corpus; so zstd-19, xz-9e e bzip2-9 (32 MiB) amostram. A diferenca entre o censo e a medida inteira (quadro unico) e a penalidade de bloco de 1 MiB que o motor paga, nao erro de amostragem: ela e a mesma ordem de grandeza da tabela de penalidade de bloco do README.

### Por arquivo (zstd-3 e xz-9e)

| Arquivo | Classe (ferramenta) | Classe (medicoes) | Codec | Razao amostra | Razao censo | Razao medicoes bloco (8 MiB) | Razao medicoes inteira | Erro vs censo % | Erro vs inteira % |
|---|---|---|---|---|---|---|---|---|---|
| E.coli | sequencia | dna | zstd-3 | 3,310 | 3,310 | 3,310 | 3,332 | 0,000 | -0,64 |
| E.coli | sequencia | dna | xz-9e | 3,797 | 3,797 | 3,797 | 3,911 | 0,000 | -2,91 |
| aleatorio.bin | criptografado_aleatorio | sint_incompressivel | zstd-3 | 1,000 | 1,000 | 1,000 | 1,000 | 0,000 | 0,00 |
| aleatorio.bin | criptografado_aleatorio | sint_incompressivel | xz-9e | 1,000 | 1,000 | 1,000 | n/d | 0,000 | - |
| bible.txt | texto | texto | zstd-3 | 3,411 | 3,411 | 3,411 | 3,455 | 0,000 | -1,27 |
| bible.txt | texto | texto | xz-9e | 4,394 | 4,311 | 4,311 | 4,570 | 1,930 | -3,85 |
| dickens | texto | texto | zstd-3 | 2,731 | 2,731 | 2,738 | 2,782 | 0,000 | -1,85 |
| dickens | texto | texto | xz-9e | 3,220 | 3,256 | 3,268 | n/d | -1,131 | - |
| ja_comprimido.xz | container_comprimido | sint_ja_comprimido | zstd-3 | 1,000 | 1,000 | 1,000 | 1,000 | 0,000 | 0,00 |
| ja_comprimido.xz | container_comprimido | sint_ja_comprimido | xz-9e | 1,000 | 1,000 | 1,000 | 1,000 | 0,000 | 0,01 |
| logs.log | log | sint_logs | zstd-3 | 4,046 | 4,046 | 4,048 | 4,079 | 0,000 | -0,81 |
| logs.log | log | sint_logs | xz-9e | 5,416 | 5,416 | 5,419 | n/d | -0,016 | - |
| mozilla | binario | binario | zstd-3 | 2,762 | 2,762 | 1,788 | 2,810 | 0,000 | -1,73 |
| mozilla | binario | binario | xz-9e | 3,595 | 3,567 | 1,945 | n/d | 0,789 | - |
| mr | binario | cientifico | zstd-3 | 2,807 | 2,807 | 2,842 | 2,811 | 0,000 | -0,14 |
| mr | binario | cientifico | xz-9e | 3,441 | 3,537 | 3,588 | n/d | -2,706 | - |
| nci | texto | cientifico | zstd-3 | 11,583 | 11,583 | 10,999 | 11,840 | 0,000 | -2,17 |
| nci | texto | cientifico | xz-9e | 19,621 | 18,217 | 17,155 | n/d | 7,709 | - |
| ooffice | binario | binario | zstd-3 | 1,951 | 1,951 | 1,951 | 1,968 | 0,000 | -0,86 |
| ooffice | binario | binario | xz-9e | 2,300 | 2,443 | 2,443 | 2,535 | -5,877 | -9,27 |
| osdb | binario | banco_de_dados | zstd-3 | 2,847 | 2,847 | 2,848 | 2,880 | 0,000 | -1,16 |
| osdb | binario | banco_de_dados | xz-9e | 3,325 | 3,336 | 3,340 | n/d | -0,330 | - |
| reymont | texto | texto | zstd-3 | 3,369 | 3,369 | 3,369 | 3,420 | 0,000 | -1,48 |
| reymont | texto | texto | xz-9e | 4,678 | 4,552 | 4,552 | 5,037 | 2,775 | -7,13 |
| samba | texto | codigo | zstd-3 | 4,161 | 4,161 | 3,015 | 4,360 | 0,000 | -4,57 |
| samba | texto | codigo | xz-9e | 4,740 | 5,121 | 3,371 | n/d | -7,438 | - |
| sao | binario | cientifico | zstd-3 | 1,307 | 1,307 | 1,307 | 1,312 | 0,000 | -0,37 |
| sao | binario | cientifico | xz-9e | 1,624 | 1,614 | 1,614 | 1,639 | 0,585 | -0,90 |
| webster | texto | texto | zstd-3 | 3,360 | 3,360 | 3,350 | 3,427 | 0,000 | -1,97 |
| webster | texto | texto | xz-9e | 4,153 | 4,190 | 4,183 | n/d | -0,898 | - |
| world192.txt | texto | texto | zstd-3 | 3,651 | 3,651 | 3,651 | 3,774 | 0,000 | -3,27 |
| world192.txt | texto | texto | xz-9e | 4,696 | 4,679 | 4,679 | 5,101 | 0,373 | -7,94 |
| x-ray | cientifico_numerico | cientifico | zstd-3 | 1,381 | 1,381 | 1,381 | 1,393 | 0,000 | -0,85 |
| x-ray | cientifico_numerico | cientifico | xz-9e | 1,868 | 1,868 | 1,867 | n/d | 0,000 | - |
| xml | xml_html | xml | zstd-3 | 8,191 | 8,191 | 8,191 | 8,414 | 0,000 | -2,65 |
| xml | xml_html | xml | xz-9e | 11,522 | 11,522 | 11,522 | 12,291 | 0,000 | -6,25 |


### Orcamento pequeno (16 MiB rapidos, 4 MiB lentos por classe), 8 sementes

Erro por arquivo contra o censo (so blocos sorteados de cada arquivo): zstd-1 medio 3,09% max 42,48%; zstd-3 medio 3,13% max 44,92%; zstd-9 medio 3,47% max 51,07%; zstd-19 medio 6,39% max 85,51%; xz-9e medio 6,76% max 84,21%; bzip2-9 medio 6,03% max 74,82%.

Cobertura do IC95 (a economia do censo cai dentro do intervalo): classes com 5 ou mais arquivos: zstd-1 16/16; zstd-3 16/16; zstd-9 16/16; zstd-19 10/16; xz-9e 10/16; bzip2-9 10/16. Todas as classes (inclui as de baixa confianca, com 1 a 4 arquivos): zstd-1 72/72; zstd-3 72/72; zstd-9 72/72; zstd-19 63/72; xz-9e 63/72; bzip2-9 64/72.

| Classe (ferramenta) | Arquivos | Confianca | zstd-3 economia censo % | erro abs medio pp | max pp | IC95 cobriu | xz-9e economia censo % | erro abs medio pp | max pp | IC95 cobriu |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 7 | ok | 76,7 | 1,27 | 4,02 | 8/8 | 81,7 | 4,93 | 12,68 | 6/8 |
| binario | 5 | ok | 59,4 | 3,16 | 9,85 | 8/8 | 67,9 | 5,66 | 19,82 | 4/8 |
| log | 2 | BAIXA | 75,3 | 0,01 | 0,01 | 8/8 | 81,5 | 0,02 | 0,04 | 6/8 |
| criptografado_aleatorio | 1 | BAIXA | 0,0 | 0,00 | 0,00 | 8/8 | 0,0 | 0,00 | 0,00 | 8/8 |
| cientifico_numerico | 1 | BAIXA | 27,6 | 0,00 | 0,00 | 8/8 | 46,5 | 0,34 | 0,67 | 8/8 |
| container_comprimido | 1 | BAIXA | 0,0 | 0,00 | 0,00 | 8/8 | 0,0 | 0,00 | 0,00 | 8/8 |
| xml_html | 1 | BAIXA | 87,8 | 0,00 | 0,00 | 8/8 | 91,3 | 1,03 | 1,96 | 8/8 |
| sequencia | 1 | BAIXA | 69,8 | 0,00 | 0,00 | 8/8 | 73,7 | 0,03 | 0,13 | 7/8 |
| json_ndjson | 1 | BAIXA | 68,5 | 0,00 | 0,00 | 8/8 | 68,6 | 0,00 | 0,00 | 8/8 |


### Classe da ferramenta contra o rotulo das medicoes

| Arquivo | Ferramenta | Medicoes |
|---|---|---|
| E.coli | sequencia | dna |
| aleatorio.bin | criptografado_aleatorio | sint_incompressivel |
| bible.txt | texto | texto |
| dickens | texto | texto |
| ja_comprimido.xz | container_comprimido | sint_ja_comprimido |
| logs.log | log | sint_logs |
| mozilla | binario | binario |
| mr | binario | cientifico |
| nci | texto | cientifico |
| ooffice | binario | binario |
| osdb | binario | banco_de_dados |
| reymont | texto | texto |
| samba | texto | codigo |
| sao | binario | cientifico |
| webster | texto | texto |
| world192.txt | texto | texto |
| x-ray | cientifico_numerico | cientifico |
| xml | xml_html | xml |


## B. Dataset versionado sintetico (v1/v2 de uma arvore)

204 arquivos, 285.0 MiB; a ferramenta leu 560.7 MiB (196,7% do inventario) em 18 s.

**Classificacao**: 183 de 189 arquivos com expectativa classificados como esperado (96,8%). Erros: v1/texto/dicionario/webster_2.txt esperado texto obtido xml_html; v1/texto/dicionario/webster_3.txt esperado texto obtido xml_html; v2/cientifico/sensor_1.dat esperado cientifico_numerico obtido binario; v2/cientifico/sensor_2.dat esperado cientifico_numerico obtido binario; v2/texto/dicionario/webster_2.txt esperado texto obtido xml_html; v2/texto/dicionario/webster_3.txt esperado texto obtido xml_html.

**Dedup**: ferramenta 60573953 bytes em 36 grupos; oraculo (sha256 de todos os arquivos) 60573953 bytes em 36 grupos; iguais: sim. Bytes lidos no hash completo: 114451246 de 143251422 candidatos por tamanho.

**Familias**: 52 de 52 pares v1->v2 modificados foram achados (recall 100,0%); 0 pares extras na ferramenta (razao patch/zstd-3 media -: extras tem alguma semelhanca). 52 familias, 55,3% dos bytes; pares medidos 52 de 52 possiveis; falhas de roundtrip 0.

| Classe | patch/zstd-3 (ferramenta) | IC95 | Pares medidos | patch/zstd-3 exato dos pares v1->v2 (arquivo inteiro) | Pares exatos |
|---|---|---|---|---|---|
| texto | 0,1607 | [0,1175; 0,1814] | 6 | 0,1629 | 6 |
| codigo | 0,2182 | [0,1905; 0,2485] | 26 | 0,2182 | 26 |
| csv_tabular | 0,1102 | [0,1068; 0,1149] | 4 | 0,1102 | 4 |
| json_ndjson | 0,1128 | [0,1125; 0,1130] | 2 | 0,1128 | 2 |
| log | 0,1057 | [0,1002; 0,1096] | 3 | 0,1057 | 3 |
| xml_html | 0,1259 | [0,1259; 0,1259] | 1 | 0,1259 | 1 |
| binario | 0,0845 | [0,0257; 0,1609] | 8 | 0,0845 | 8 |
| banco_de_dados | 0,1177 | [0,1177; 0,1177] | 1 | 0,1177 | 1 |
| cientifico_numerico | 0,0882 | [0,0882; 0,0882] | 1 | 0,0882 | 1 |


Variante com so 12 pares medidos (extrapolacao): ferramenta 0,0641 [0,0302; 0,1196] contra exato 0,1132; o IC95 cobre o exato: sim.

Ignorados: {'nao_regular': 1, 'link_simbolico': 4, 'sem_permissao_diretorio': 1}. Ilegiveis: {'sem_permissao_leitura': 1}.

Resumo executivo gerado (anonimizado, tambem em `perfil_exemplo.md`):

1. Dataset: 204 arquivos, 285,02 MiB em 41 diretorios (mediana 223,90 KiB, p99 12,00 MiB); classes dominantes: texto 28,0%, binario 25,1%, cientifico_numerico 10,8%.
2. Fracao incompressivel: 11,5% por classificacao (midia, containers comprimidos, criptografado/aleatorio) e 11,5% [11,5%; 11,5%] por amostra (blocos com economia zstd-3 < 3%).
3. Dedup de arquivo inteiro: 20,3% dos bytes (57,77 MiB) sao copias exatas, em 36 grupos.
4. Familias de versoes: 55,3% dos bytes em 52 familias (27,7% sao versoes nao-base); o delta (patch-from) custa 11,1% [7,0%; 14,9%] do zstd-3 isolado em 52 pares medidos.
5. Compressao do dataset (blocos de 1 MiB, ponderada por bytes): zstd-3 economiza 52,9% [50,2%; 54,7%] (razao 2,12x); zstd-19 56,9%; xz-9e 59,3% [56,6%; 61,3%].
6. Ganho por classe (zstd-3, IC95): texto 70,3% [65,1%; 72,8%]; binario 50,8% [42,6%; 56,3%]; cientifico_numerico 16,3% [10,4%; 21,9%]; xml_html 78,0% [69,3%; 85,5%].
7. Economia mensal estimada so com compressao (politica por classe): US$ 3.665.227 [US$ 3.486.138; US$ 3.788.281] = armazenamento US$ 3.675.350 + movimento US$ 0,00 - CPU US$ 10.122.
8. Com dedup e delta: US$ 4.775.620 [US$ 4.643.875; US$ 4.826.543] por mes; recompressao assincrona recomendada em: texto->xz-9e, codigo->xz-9e, csv_tabular->xz-9e, json_ndjson->xz-9e, log->xz-9e, xml_html->xz-9e, binario->xz-9e, banco_de_dados->xz-9e, cientifico_numerico->xz-9e; stored em: imagem_audio_video, criptografado_aleatorio.
9. Parametros (premissas): volume 5,00 PB/mes, fracao retida 0,90, retencao 60 meses, armazenamento US$ 0,0230/GB-mes, nucleo US$ 0,0446/h, 3 leituras/GB, custo de movimento 0,00 (DESCONHECIDO).
10. Limites: amostra de blocos de 1 MiB e heuristicas de classe e familia; vazoes de outro corpus/CPU; desconhecido: o que o cliente chama de processamento e se o consumo aceita dado comprimido (ver secao de limites).
