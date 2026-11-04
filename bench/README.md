# bench: fronteira economica de compressao sem perdas (razao x custo de CPU x valor do armazenamento)

Benchmark reprodutivel que mede razao e vazao por nucleo de compressores classicos sobre dados publicos e os converte em US$/GB, numero de nucleos para 5 PB/mes e ponto de equilibrio de recompressao em camadas. Benchmarks comuns (lzbench, Squash) param em razao e MB/s; aqui o resultado final e dinheiro.

Convencao do projeto: o codigo em `bench/*.py` nao tem comentarios; todo o "porque" esta neste arquivo.

## Como rodar

Preparar dados (baixa Silesia e Canterbury Large, extrai cada um em diretorio novo e vazio, gera os dados sinteticos, grava `manifesto.json` com tamanho e sha256):

```
python3 -I bench/preparar_dados.py --destino DADOS --zips ZIPS
```

Coletar medicoes (tudo de ponta a ponta: memoria, principal, dicionario, P-core vs E-core, escala multi-nucleo):

```
python3 -I bench/syntrabench.py --corpus-dir DADOS --saida bench/resultados/medicoes.json --repeticoes 3 --cpu 0,2,4,6
```

Modelo economico e resumo:

```
python3 -I bench/economia.py
python3 -I bench/economia.py --autoteste
```

Rodada 2 (F1: dedup por bloco; escala em nucleos homogeneos). Todos os comandos usam `python3 -I`; `DADOSF1` e o diretorio de trabalho do F1 (tmpfs, fora de `bench/`), `SILESIA` e o diretorio `silesia/` da rodada anterior:

```
python3 -I bench/dedup.py preparar --dados DADOSF1 --silesia SILESIA --saida bench/resultados/dedup.json
python3 -I bench/dedup.py sanidade --saida bench/resultados/dedup.json
python3 -I bench/dedup.py analise --dados DADOSF1 --saida bench/resultados/dedup.json --workers 16 --amostra19 4
python3 -I bench/dedup.py delta --dados DADOSF1 --saida bench/resultados/dedup.json --workers 16
python3 -I bench/dedup.py long --dados DADOSF1 --saida bench/resultados/dedup.json
taskset -c 0 python3 -I bench/dedup.py vazao --dados DADOSF1 --saida bench/resultados/dedup.json --cpu 0 --repeticoes 3 --fatia-mib 256
python3 -I bench/dedup.py relatorio --saida bench/resultados/dedup.json --md bench/resultados/dedup.md
```

`preparar` baixa os tres `tar.xz` do kernel (confere o sha256 contra `sha256sums.asc` do mesmo diretorio), extrai cada um em diretorio novo e vazio com `tarfile` e `filter="data"`, gera o tar deterministico, apaga a arvore extraida e gera o fluxo sintetico (`--versoes` restringe quais versoes preparar). `analise` e `delta` usam varios processos (a medicao de tempo so existe em `vazao`, que roda sozinha e fixada em um P-core). O compilador (`gcc -O3 -shared -fPIC`, sem `-march=native`) e chamado em tempo de execucao para um diretorio temporario (`TMPDIR`).

Escala em nucleos homogeneos (so `escala`; a saida NAO pode ser `medicoes.json`: o programa recusa):

```
python3 -I bench/syntrabench.py --corpus-dir DADOS --saida bench/resultados/escala_homogenea.json --fase escala --repeticoes 3 --escala-duracao 1.5 --cpu 0 --cpu-e 16 --escala-cpus "E=16-31@1,2,4,8,12,16;P=0,2,4,6,8,10,12,14@1,2,4,8;PSMT=0,2,4,6,8,10,12,14,1,3,5,7,9,11,13,15@8,16;PE=0,2,4,6,8,10,12,14,16,17,18,19,20,21,22,23@16"
python3 -I bench/escala_resumo.py --entrada bench/resultados/escala_homogenea.json --medicoes-anteriores bench/resultados/medicoes.json --md bench/resultados/escala_homogenea.md
```

Formato de `--escala-cpus`: grupos separados por `;`, cada um `rotulo=lista@n1,n2,...`; `lista` aceita CPUs e intervalos (`16-31`) e a ORDEM importa: o ponto N usa as N primeiras CPUs da lista, um processo pinado por CPU. Sem `@`, vale `--escala-n`. `--cpu` (primeiro valor) e o P-core de referencia isolado, `--cpu-e` o E-core de referencia isolado.

`DADOS` deve ficar em tmpfs (elimina I/O de disco) e fora de `bench/`. Rode sempre com `python3 -I` (modo isolado: nao carrega nada do diretorio de dados nem do diretorio corrente). Os dados baixados sao tratados como nao confiaveis: so sao lidos como bytes, nunca executados; o zip e extraido somente para nomes esperados (lista fixa), com limite de tamanho.

Argumentos de `syntrabench.py`: `--corpus-dir`, `--saida`, `--repeticoes` (minimo 3, reporta mediana), `--cpu` (lista de CPUs logicas, um trabalhador pinado por CPU na fase principal), `--cpu-e` (CPU de E-core do subconjunto), `--cap-mib` (fatia maxima por arquivo, padrao 8), `--fase` (`tudo`, `memoria`, `principal`, `complemento`, `dicionario`, `ecore`, `escala`; `complemento` reexecuta so as celulas de zstd-3-long27 e do arquivo completo), `--escala-n`, `--escala-duracao`, `--arquivos` (filtro para teste rapido), `--escala-cpus` (listas explicitas de CPUs para a escala, ver Rodada 2; o programa recusa gravar sobre uma saida que ja contem a fase principal, como `medicoes.json`).

## Arquivos

- `preparar_dados.py`: download, extracao segura, manifesto.
- `gerar_sinteticos.py`: classes sinteticas (aleatorio, logs, ja comprimido).
- `syntrabench.py`: coleta (todas as fases; `--escala-cpus` para escala em listas explicitas de CPUs).
- `dedup.py`, `cdc.c`: gate F1 (dedup por bloco CDC e tamanho fixo contra arquivo inteiro; preparo de dados, sanidade, analise, delta por caminho, vazao, relatorio). `cdc.c` (FastCDC) e compilado em tempo de execucao com `gcc -O3 -shared -fPIC` e chamado por `ctypes`.
- `escala_resumo.py`: tabelas de `resultados/escala_homogenea.md` a partir de `escala_homogenea.json`.
- `capacidade_local.py`, `membw.c`: Rodada 4 (capacidade local sustentada: inventario, calibracao de 1 nucleo, memoria, teste sustentado de 10 min, I/O de NVMe, resumo). Saidas: `resultados/capacidade_local.json`, `resultados/capacidade_local.md`. `membw.c` e compilado em tempo de execucao com `gcc -O3 -fno-tree-loop-distribute-patterns -pthread` (sem `-march=native`).
- `plano_bloco.py`: Rodada 3 (classificador de plano por bloco: rotulagem com bit-exatidao, features, CART, regressao logistica, regra manual, avaliacao leave-one-file-out/leave-one-class-out, custo de features, criterio de parada). Saidas: `resultados/plano_bloco.json`, `resultados/plano_bloco.md`, `resultados/plano_bloco_rotulos.npz`.
- `economia.py`: modelo economico, analise marginal, regra de recompressao, requisito, gera `economia.json` e `resumo.md`.
- `precos.json`: todos os parametros, cada um com `valor`, `unidade`, `fonte` e `tipo` (`verificado` ou `inferencia_propria`).
- `resultados/medicoes.json`, `resultados/economia.json`, `resultados/resumo.md`.
- `resultados/dedup.json`, `resultados/dedup.md` (F1), `resultados/escala_homogenea.json`, `resultados/escala_homogenea.md` (escala em nucleos homogeneos).

## Metodologia

### Dados
- Silesia (12 arquivos) e Canterbury Large (bible.txt, E.coli, world192.txt), baixados de `sun.aei.polsl.pl` e `corpus.canterbury.ac.nz`. Sha256 de cada arquivo inteiro e de cada fatia medida estao em `medicoes.json` (`arquivos`) e uma versao abreviada em `resumo.md`.
- Classes: texto = dickens, webster, reymont, bible.txt, world192.txt; codigo = samba; binario = mozilla, ooffice; cientifico = sao, nci, x-ray, mr; banco_de_dados = osdb; xml = xml; dna = E.coli; `mix_real` = todos os arquivos reais juntos, ponderado por bytes.
- Classes SINTETICAS (marcadas `sintetico: true`): `sint_incompressivel` = `random.Random(20261103).randbytes(32 MiB)`; `sint_logs` = 64 MiB de linhas de log de aplicacao geradas por `gerar_sinteticos.py` (timestamps crescentes, IPs com distribuicao zipfiana, ids aleatorios de requisicao, niveis, caminhos, status; semente 1); `sint_ja_comprimido` = `xz -9` do maior arquivo de texto (webster). Os logs sinteticos tem estrutura regular e campos aleatorios inventados por mim: a razao deles NAO representa logs reais de producao.
- Fatia: cada arquivo e truncado nos primeiros 8 MiB (`--cap-mib`) em todas as configuracoes, para que as razoes sejam comparaveis entre configuracoes. Foi o corte necessario para caber no orcamento de tempo (zstd-19/22 e xz-9e rodam a poucos MB/s). Arquivos menores que 8 MiB sao usados inteiros. Um complemento (`completo` em `medicoes.json`) mede zstd-1/3/3-long27/9 nos arquivos maiores que a fatia, inteiros, para ver o efeito de `--long=27`; esse complemento nao entra no modelo economico.

### Configuracoes
zstd 1, 3, 9, 19 e 22 (`--ultra`); zstd-3 com `--long=27` (janela 2^27 e long-distance matching, so em modo inteiro); xz 1, 6, 9e; bzip2 -9; zlib 1, 6, 9; stored (copia de memoria, sem transformacao). Todas via biblioteca em processo (modulo `compression.zstd` do Python 3.14, `lzma`, `bz2`, `zlib`), que usa as mesmas bibliotecas do CLI. Motivo: e a unica forma de verificar sha256 do original contra o descomprimido em toda medicao sem cronometrar o hash. Conferencia cruzada contra `zstd -b` do CLI na secao 14 de `resumo.md`.

Modos: `inteiro` (um quadro por arquivo), `bloco_1MiB` e `bloco_64KiB` (blocos independentes, cada bloco comprimido sozinho, como em armazenamento com acesso aleatorio). Modos em bloco nao rodam todas as configuracoes (corte de tempo): 1 MiB = stored, zstd-1/3/9/19/22, xz-9e, bzip2-9, zlib-6; 64 KiB = idem sem zstd-22. Em blocos o xz usa dicionario LZMA2 do tamanho do bloco (com o padrao de 64 MiB a alocacao por bloco domina o tempo). O zstd usa os padroes da biblioteca: sem checksum de conteudo (o CLI liga o checksum por padrao, custo pequeno e no mesmo sentido para todos).

### Protocolo de tempo
- Dados em tmpfs, aquecimento (compressao+descompressao de 1 MiB, nao cronometrada), 3 repeticoes de compressao e 3 de descompressao, mediana (min e max tambem gravados). So a compressao/descompressao e cronometrada (`perf_counter_ns`); o sha256 fica fora do cronometro e e conferido em TODA repeticao de descompressao: sha256(original) == sha256(descomprimido). Qualquer divergencia entra em `bit_exatidao_violada`.
- Fixacao de CPU com `os.sched_setaffinity` (equivalente a `taskset -c`).
- A fase principal roda 4 trabalhadores pinados em 4 P-cores fisicos distintos (CPUs 0,2,4,6) consumindo uma fila de celulas (arquivo x modo x configuracao), para caber no tempo. Isso contamina a medicao com contencao de potencia/cache/memoria entre os 4 nucleos. O vies foi medido: o subconjunto dickens+mozilla foi repetido num unico P-core (CPU 0) isolado e comparado celula a celula (secao 13 de `resumo.md`, "vies de concorrencia"). O mesmo subconjunto roda isolado em um E-core (CPU 16).
- Vazao por classe = soma dos bytes originais / soma das medianas de tempo dos arquivos da classe (media harmonica ponderada por bytes).

### Escala multi-nucleo
N processos independentes (N = 1, 2, 4, 8, 16, 24, 32), cada um com dados distintos (blocos de 1 MiB de posicoes diferentes do pool de arquivos reais, passo 32), sincronizados por barreira, cada um comprimindo (ou descomprimindo) em laco por 1,5 s; 1 rodada de aquecimento descartada + 3 rodadas medidas, mediana. Os dados do pool sao amostrados de forma sistematica, entao cada trabalhador recebe uma composicao parecida. Ordem de fixacao: 8 P-cores fisicos (CPUs 0,2,...,14), depois 16 E-cores (16..31), depois os irmaos SMT dos P-cores (1,3,...,15). Assim N=8 e puro P-core, N=24 usa os 24 nucleos fisicos (8P+16E) e N=32 usa as 32 threads.

Eficiencia de escala = agregado medido / soma, sobre os trabalhadores, da vazao ideal de cada trabalhador. A vazao ideal vem da mediana por bloco medida com o P-core CPU 0 sozinho, somada sobre os blocos daquele trabalhador. Para N=1 ela vale ~1; ela mistura quatro efeitos: contencao de memoria/cache, queda de clock por potencia (turbo de todos os nucleos), E-cores mais lentos que P-cores e SMT. Nao e eficiencia de paralelismo "pura". Foi medida so para zstd-3 e zstd-1 em blocos de 1 MiB; o modelo aplica a curva do zstd-3 a todas as configuracoes (INFERENCIA PROPRIA; configuracoes mais pesadas de CPU tendem a escalar melhor porque pressionam menos a memoria) e extrapola com valor constante acima de N=32.

### Memoria
`/usr/bin/time -v` sobre o CLI (zstd 3/9/19/22, zstd-3 --long=27, xz -9e), entrada de 32 MiB de mozilla, compressao e descompressao em `taskset`. O RSS de zstd e xz cresce com o tamanho da janela/entrada; o valor e para 32 MiB, nao para o pior caso.

### Energia
RAPL em `/sys/class/powercap/intel-rapl*/energy_uj` nao e legivel sem sudo nesta maquina: ENERGIA NAO MEDIDA. Nada de J/GB foi calculado. O modelo on-prem usa so a premissa de 5 W/nucleo x PUE 1,5 (INFERENCIA PROPRIA).

### Dicionario
`zstd --train --maxdict=110000` (CLI) sobre amostras de 64 KiB do subconjunto de treino, em texto/codigo/xml/logs. Treino e teste sao regioes contiguas e disjuntas do mesmo arquivo (primeiros 40% treino, 60% finais teste, dentro de uma regiao de ate 16 MiB); para logs, o treino e um arquivo gerado com outra semente. Mesma fonte para treino e teste e otimista: em producao o dicionario envelhece. O ganho e medido contra zstd sem dicionario nas MESMAS fatias de teste. O custo de treino, versionamento e armazenamento de dicionarios nao entra.

### Dedup por bloco, gate F1 (`dedup.py`, `cdc.c`)

Pergunta: abandonar o CDC se o ganho extra sobre o dedup de arquivo inteiro for < 5% nos dados-alvo ou se o indice nao couber na RAM.

**Dados (publicos, reprodutiveis).** Kernel Linux de `https://cdn.kernel.org/pub/linux/kernel/v6.x/` (listagem consultada em 2026-11-03: a serie 6.x chega a 6.19.14). Par A (patch): 6.12.114 -> 6.12.115 (releases estaveis consecutivos). Par B (minor): 6.12.115 -> 6.13.12 (series consecutivas, ultima versao de cada uma presente na listagem no momento em que escolhi). O sha256 de cada `tar.xz` foi conferido contra `sha256sums.asc` do mesmo diretorio (so o sha256; a assinatura PGP NAO foi verificada) e esta em `dedup.json` (`dados.versoes_kernel`). Cada versao existe em duas representacoes: (i) ARVORE: arquivos regulares nao vazios (23 a 28 arquivos vazios por versao ficam fora, ocupam 0 byte), cada arquivo e um objeto; (ii) FLUXO TAR: um unico arquivo grande (1,55 GB), GNU tar, nomes em ordem, `mtime`/uid/gid zerados, modo 0644/0755, prefixo comum `linux/` nas duas versoes (equivale a fazer backup do mesmo diretorio duas vezes; com o numero de versao no caminho todos os cabecalhos mudariam). O tar tem tambem 5.761 diretorios e 62 symlinks que a arvore nao conta.

Detalhe de armazenamento (DESVIO do pedido, imposto pelo ambiente): o `/tmp` tem quota de usuario (`usrquota`) e a primeira tentativa de extrair as tres arvores em paralelo estourou com `EDQUOT` mesmo com `df` mostrando 11 GB livres; medi cerca de 7,5 GiB de folga real. Por isso cada arvore e extraida (`tarfile`, `filter="data"`), o tar e gerado a partir dela, e a arvore e APAGADA em seguida (permitido pelo pedido: "apague as arvores"). A representacao ARVORE da analise le cada arquivo como fatia do tar por offset gravado em `manifesto-<versao>.json`; antes de apagar a arvore o programa confere o sha256 de cada fatia contra o sha256 do arquivo extraido (`offsets_no_tar_conferem_com_sha256_dos_arquivos` = true nas 3 versoes). Os bytes sao os mesmos que a arvore teria.

**SINTETICO (rotulo obrigatorio).** "Backup 1" = os 12 arquivos do Silesia concatenados em ordem alfabetica (211.938.580 B). "Backup 2" (semente 20261103, `numpy.random.default_rng`): 835 edicoes in-place de 1 a 4 KiB (2.120.332 B = 1,00%) com bytes aleatorios, 40 insercoes de 20 a 2000 B (44.155 B) em posicoes aleatorias com bytes aleatorios, e 1,00% de bytes aleatorios anexados (2.119.385 B). Os bytes aleatorios sao incompressiveis, o que deixa a razao de compressao do backup 2 um pouco pior que a de dados novos reais. So existe como fluxo (nao ha arvore nem dedup de arquivo inteiro).

**Chunkers** (todos deterministicos, devolvem lista de (offset, tamanho)): tamanho fixo 4 KiB, 8 KiB, 64 KiB, 1 MiB; FastCDC normalizado (Xia et al., USENIX ATC 2016) com medias nominais de 8 KiB, 64 KiB e 1 MiB, em `cdc.c`: hash Gear de 64 bits `fp = (fp << 1) + G[byte]`, tabela G de 256 inteiros gerada por `numpy.random.default_rng(20261103)`; `min = medio/4`, `max = medio*8`, os primeiros `min` bytes de cada chunk nao sao examinados (cut-point skipping, fp zerado ao iniciar o chunk); normalizacao nivel 2: ate `medio` bytes usa a mascara S com `b+2` bits (corte mais dificil), depois a mascara L com `b-2` bits (corte mais facil), `b = log2(medio)`. As mascaras sao aleatorias (permutacao de semente 1234567 das posicoes de bit 16 a 63, S contem L), porque as mascaras literais do paper dependem do tamanho medio especifico; isso e uma escolha minha, nao copia do codigo de referencia. O que sobra no fim do arquivo (<= min) vira o ultimo chunk. Na arvore, cada arquivo e chunkeado separadamente (um arquivo menor que `min` e um chunk so); no tar e no sintetico, o fluxo inteiro e chunkeado de uma vez.

**Hash.** `hashlib.blake2b(digest_size=32)`, SUBSTITUTO de BLAKE3 (`import blake3` nao existe aqui). A identidade de um chunk sao os 32 bytes inteiros. A vazao de BLAKE3 nao foi medida e deve ser maior.

**Metricas.** Para cada (conjunto, estrategia), v1 e armazenado primeiro e v2 depois:
- `% de v2 ja em v1` = bytes de v2 cujo chunk (ou arquivo) tem hash presente no conjunto de chunks unicos de v1, dividido pelos bytes de v2. (`pct_v2_duplicados_total` em `dedup.json` tambem conta duplicatas dentro do proprio v2.)
- Armazenado total (v1+v2) = soma, sobre os chunks unicos de v1 uniao v2, do tamanho bruto, do tamanho do quadro zstd-3 (cada chunk comprimido sozinho, como um quadro independente: o cabecalho de cerca de 9 a 13 B por quadro esta incluso) e do tamanho do quadro zstd-19. Receitas (lista de referencias por arquivo) e nomes NAO entram nesse numero; a coluna "ganho c/ metadados" do `dedup.md` soma 32 B por referencia de chunk e 32 B por chunk unico (INFERENCIA PROPRIA sobre o custo de metadados) e quase nao muda o veredito.
- Razao total = (logico v1 + logico v2) / armazenado total.
- **Baselines do gate:** na arvore, "arquivo inteiro + zstd-3" = cada arquivo unico (hash do conteudo) comprimido com zstd-3 sozinho; no fluxo (tar, sintetico), onde dedup de arquivo inteiro nao existe, "zstd-3 do fluxo inteiro, sem dedup" = v1 e v2 comprimidos cada um como um quadro zstd-3 (janela de 2 MiB do nivel 3: sem referencia a v1).
- **Ganho extra do CDC** = (armazenado do baseline - armazenado CDC+zstd-3) / armazenado do baseline. Limite do gate: 5%. Negativo = o CDC armazena MAIS que o baseline.
- zstd-19 (so CDC 64K e 1M): ESTIMATIVA por amostra, porque zstd-19 roda a poucos MB/s; 1 em cada 4 chunks unicos (os que tem `primeiro byte do hash mod 4 == 0`) e comprimido tambem com zstd-19, e `armazenado_z19 = armazenado_z3 x (soma z19 da amostra / soma z3 da amostra)` (estimador de razao de somas). Nao ha baseline zstd-19 de arquivo inteiro; o gate e so zstd-3.
- Extra `fluxo_z3_long31_concatenado` (so fluxo): v1+v2 concatenados em um fluxo zstd-3 com janela de 2 GiB e long-distance matching. Nao e dedup por chunk (nao tem indice) e nao tem acesso aleatorio; serve de referencia do que se obtem sem nenhum indice.
- **Delta por caminho (extra, "limite superior de dedup por similaridade")**: para cada arquivo de v2 com o mesmo caminho em v1 e conteudo diferente, tamanho do quadro zstd-3 com v1 como prefixo (`ZstdDict(is_raw=True).as_prefix` de `compression.zstd`, `window_log` ajustado ao tamanho; equivale a `zstd --patch-from`), conferido por round-trip; comparado com o custo marginal CDC+zstd-3 dos mesmos arquivos (chunks unicos que aparecem pela primeira vez nesses arquivos de v2). Conferencia com o CLI `zstd --patch-from=... -3 --no-check` em 40 arquivos por par esta em `dedup.json` (`conferencia_cli_patch_from`). Arquivos novos (caminho inexistente em v1) pagam zstd-3 sozinhos (com dedup de conteudo) no total "delta por caminho".
- **Indice**: bytes teoricos = chunks unicos x 32 B (so hash; com localizacao de 8 B seriam 40 B, campo `hash_mais_local_40B` em `dedup.json`). **Extrapolacao para 5 PB/mes** (INFERENCIA PROPRIA): chunks unicos por byte logico observados no par (n_unicos / (logico v1 + v2)) x 5e15 B x 32 B = TB de indice por mes de dado ingerido; "meses em 1 TB / 4 TB de RAM" = 1 ou 4 / (TB por mes), supondo que o indice de todos os meses retidos precisa estar na RAM e que a taxa de dedup do par se repete (com mais versoes retidas a taxa subiria e o indice seria menor que o extrapolado; com dados sem redundancia seria maior: `pior_caso_sem_dedup_TB_por_mes` em `dedup.json`).
- **Vazao** (`vazao`): 1 processo fixado em CPU 0 (P-core fisico; `taskset -c 0` mais `os.sched_setaffinity`), GC desligado, fatia = primeiros 256 MiB do tar de 6.12.115 em mmap (page cache quente), 3 repeticoes, mediana, MB = 1e6 B. Chunker = so a chamada de chunking; hash = `blake2b` por chunk sobre os chunks ja calculados; chunker+hash+lookup = chunking + blake2b + consulta (e insercao se ausente) em um `dict` do Python pre-preenchido com os chunks dos primeiros 256 MiB do tar de 6.12.114. O `dict` do Python NAO representa um indice de producao (ele e RAM pura, sem I/O, sem concorrencia, sem persistencia). Nos chunkers de tamanho fixo o "chunker" e aritmetica de offsets e nao tem significado como vazao.
- **Bit-exatidao** (todas as estrategias, todos os conjuntos): (1) cada chunk unico novo e comprimido com zstd-3, descomprimido e comparado byte a byte com o original (tambem na amostra zstd-19); (2) cada arquivo da arvore de v2 e o tar/backup 2 inteiro sao RECONSTRUIDOS concatenando os chunks na ordem da receita (cada chunk e buscado pelo hash no local da primeira ocorrencia, em v1 ou em v2) e o sha256 do resultado e comparado ao sha256 registrado no preparo; (3) o baseline de fluxo e comprimido, descomprimido e conferido por sha256. Limite: nao existe um repositorio de chunks comprimidos em disco; a reconstrucao le o chunk original no local registrado (o que prova que identidade de hash implica identidade de bytes e que a receita cobre o arquivo, e o codec e validado em separado).

**Testes de sanidade** (`dedup.py sanidade`, semente 424242): (a) media de chunk em 256 MiB aleatorios; (b) 64 MiB aleatorios + 100 bytes inseridos no inicio, fracao dos chunks do original cujo hash reaparece; (c) round-trip: offsets contiguos cobrindo o arquivo, concatenacao igual ao original e sha256 igual, no original e no modificado; mais determinismo (duas execucoes iguais). Resultados na secao de resultados abaixo.

### Escala em nucleos homogeneos (`--escala-cpus`)

Mesmo trabalhador, mesmo protocolo da escala anterior (processos independentes, blocos de 1 MiB distintos por processo e fixos entre pontos N pelo mesmo passo de 32 blocos, barreira, rodadas de 1,5 s, 1 rodada de aquecimento descartada, mediana de 3, bit-exatidao conferida no setup de cada trabalhador). Novidade: a vazao isolada de referencia e medida em DOIS nucleos, o P-core CPU 0 e o E-core CPU 16, no pool inteiro de blocos; para cada trabalhador a vazao ideal vem do mesmo tipo de nucleo. Cada ponto traz duas eficiencias: "vs mesmo tipo" (agregado / soma das vazoes isoladas do mesmo tipo de nucleo: mede so contencao e queda de clock; para E-cores e a "eficiencia relativa a 1 E-core") e "vs P-core" (a definicao da curva anterior, que inclui o fato de o E-core ser mais lento que o P-core). Grupos: (a) E-cores fisicos CPUs 16..31 N em {1,2,4,8,12,16}; (b) P-cores fisicos 0,2,...,14 N em {1,2,4,8}; (c) P-cores com SMT, lista 0,2,...,14,1,3,...,15, N em {8,16} (N=8 repete (b), serve de repetibilidade; N=16 usa as 16 threads); (d) 8 P fisicos + E-cores 16..23, N=16 (mesmas CPUs do N=16 da curva anterior). Os E-cores 16..31 sao 16 nucleos fisicos (sem SMT) em 4 clusters de 4 que dividem 4 MiB de L2 (confirmado em `/sys/devices/system/cpu/cpu*/cache/index2/shared_cpu_list`; cada P-core fisico tem 2 MiB de L2 proprio), entao o L2 compartilhado de cada cluster entra na medida e NAO e separavel da banda de memoria e do L3 com os dados desta rodada.

## Definicao das metricas

- Razao x = bytes originais / bytes comprimidos. s = fracao economizada = 1 - comprimido/original. Se s <= 0 o sistema armazena cru: s_efetivo = 0 e o custo de descompressao e zero (a tentativa de compressao continua custando).
- theta_c, theta_d = vazao por nucleo de compressao e descompressao, MB/s (10^6 B/s).
- Penalidade de bloco = (tamanho comprimido em blocos / tamanho comprimido inteiro - 1) em %.
- Ganho de dicionario = razao com / razao sem - 1 (em %), e reducao de tamanho = 1 - comprimido_com / comprimido_sem.
- Eficiencia de escala: ver acima.

## Modelo economico (`economia.py`, parametros em `precos.json`)

1. rho = 5e15 B / (30 x 86400 s) = 1,929 GB/s (media); rho_pico = k x rho, k=4 => 7,716 GB/s. N = rho_x / (theta x fator_servidor x eficiencia(N)), resolvido por ponto fixo com a eficiencia medida interpolada linearmente em N (constante acima de 32). N_pico e arredondado para cima para checar encaixe em 64 nucleos e em 1 rack (64 x 20 nucleos, INFERENCIA PROPRIA).
2. Custo mensal de computar = N_medio x preco_por_nucleo_hora x 730 h. Cenarios: NUVEM (c7i vCPU US$0,0446/h, Vantage 2026-11-03, dado no pedido) e ON-PREM (US$0,0135/nucleo-h, INFERENCIA PROPRIA: servidor ~US$15k/64 nucleos, 3 anos, 70% de uso, 5 W/nucleo x PUE 1,5 x US$0,10/kWh; a conta bate: US$0,01274 + US$0,00075 = US$0,01349). Tambem e dado o custo provisionado no pico (N_pico inteiro x preco x 730 h), para quem nao tem elasticidade.
3. Valor do armazenamento por GB = s x p_gb_mes x T x redundancia (T = 60 meses, redundancia 1,0, sem desconto temporal e sem queda futura de preco). p_gb_mes: S3 Standard 0,023; B2 0,00695; Glacier IR 0,004; on-prem tudo-incluido 0,002 (INFERENCIA PROPRIA). Cenario = CPU x armazenamento (8 combinacoes; resumo destaca nuvem+s3_standard, nuvem+glacier_ir, onprem+onprem).
4. Custo de CPU por GB = preco_nucleo_h / (theta x fator x eficiencia x 3,6) (1 MB/s = 3,6 GB/h). Camadas de leitura: quente n=100, morno n=5, frio n=0,1 leituras por coorte durante a vida. O custo mensal do item 2 e o custo por GB do item 4 diferem 730/720 = 1,4% por usar 730 h contra um mes de 30 dias.
5. Valor liquido por GB = valor de armazenamento - custo de compressao - n x custo de descompressao; mensal da coorte = x 5e6 GB. `stored` tem custo de CPU 0 (nenhuma transformacao; o custo de copia/I/O existe em qualquer configuracao e nao e medido).
6. Marginal: configuracoes ordenadas por razao; para cada passo, ganho de armazenamento, custo marginal de CPU e delta liquido. Otimo por (classe, camada, cenario) = maior valor liquido. Pareto: configuracao nao dominada em (razao, vazao de compressao); tambem marcada para descompressao.
7. Recompressao: dado ja em zstd-1/3 na ingestao, reempacotar para o destino D ao custo c_d(origem) + c_c(D) por GB (eficiencia de servidor cheio, N=32). Com taxa de leitura r = n/T por mes, o beneficio por mes restante e delta_s x p - r x (c_d(D) - c_d(origem)). Ponto de equilibrio M* = custo de reempacotar / beneficio mensal liquido; se o beneficio e <= 0, "nunca" (a leitura extra custa mais que o armazenamento poupado). Tambem e gravado o numero maximo de leituras na vida restante (60, 36, 12 meses) que ainda compensa.
8. Requisito: para cada configuracao, N_pico, se cabe em 64 nucleos e em 1 rack, e custo/mes. Destaque das que cumprem 5 PB/mes com N_pico <= 64.

## Validacao do modelo a mao

Caso: zstd-1 a 510 MB/s por nucleo, eficiencia de escala 1. A mao: rho = 5e15 / 2.592.000 s = 1.929,01 MB/s; N medio = 1.929,01 / 510 = 3,782 nucleos (3,8); N pico = 4 x 3,782 = 15,13, ou seja 16 nucleos; custo nuvem = 3,7824 x 0,0446 x 730 = US$123,15/mes; on-prem = 3,7824 x 0,0135 x 730 = US$37,28/mes; custo de compressao nuvem = 0,0446 / (510 x 3,6) = US$2,43e-5/GB. O programa (`python3 -I bench/economia.py --autoteste`) devolve N medio 3,78238, N pico 15,1295 (16), nuvem US$123,147, on-prem US$37,275, US$2,429e-5/GB: confere. O mesmo teste verifica a derivacao do preco on-prem (US$0,01349 contra US$0,0135 do JSON). O teste roda a cada execucao de `economia.py`.

## LIMITACOES

- CPU de notebook hibrido (i9-14900HX: 8 P-cores a ate 5,6-5,8 GHz + 16 E-cores a 4,1 GHz, governor `powersave`, termica e limite de potencia de notebook) nao e nucleo de servidor. O vCPU de um c7i (Sapphire Rapids, uma thread SMT, clock menor) deve render menos que um P-core Raptor Cove; o modelo assume fator 1,0 (`fator_vazao_servidor_vs_notebook`), provavelmente otimista. Sensibilidade: N e custo escalam por 1/fator (fator 0,6 => 1,67x mais nucleos e custo).
- Eficiencia de escala medida em 32 threads de um unico processador, so zstd-1/zstd-3; extrapolada para 64 nucleos e para as demais configuracoes.
- Tudo em tmpfs: sem I/O de disco, de rede nem de camada de armazenamento; o custo de copia de memoria, de metadados, de checksums de integridade e de erasure coding nao esta incluido; nao ha latencia de cauda nem fila.
- Dedup e CDC so foram medidos na Rodada 2 (secao final), em codigo-fonte do kernel Linux (2 pares) e em um par SINTETICO sobre o Silesia; NAO representam VM, banco de dados nem backups reais, e o modelo economico (`economia.py`) continua sem dedup. O hash do dedup e blake2b-256 (nao BLAKE3), o indice medido e um `dict` do Python (nao e indice de producao) e o gate F1 usa zstd-3 por chunk. Limitacoes completas na secao de falhas da Rodada 2.
- Compressao neural NAO foi medida (a pesquisa previa mostrou 0,0001-4 MB/s por GPU, tres ordens de grandeza abaixo do requisito).
- A medicao principal usa 4 trabalhadores concorrentes (ver vies medido) e fatia de 8 MiB por arquivo; razoes de arquivos grandes (mozilla 51 MB, nci 33 MB, webster 41 MB) em fatia de 8 MiB nao sao as do arquivo inteiro.
- Classes sinteticas nao representam producao: logs com campos aleatorios inventados, aleatorio e ja comprimido servem de limite superior/inferior.
- Precos: valores de tabela informados no pedido, nao reverificados nesta execucao; on-prem e INFERENCIA PROPRIA; sem desconto temporal, sem queda de preco, sem custo de recuperacao do Glacier IR, sem custo de saida de dados.
- Versoes: zstd 1.5.7 (CLI e biblioteca), xz/liblzma 5.8.3, zlib 1.3.1, bzip2 1.0.8, Python 3.14.4. Resultados valem para essas versoes e flags (sem checksum no zstd, `xz` com CRC64).
- Energia nao medida (RAPL ilegivel sem sudo).
- O modelo de leitura usa n leituras por coorte durante a vida inteira, uniformes no tempo; nao ha modelo de popularidade por objeto.

## Resultados desta execucao (resumo; tabelas completas em `resultados/resumo.md`, numeros brutos em `resultados/medicoes.json` e `resultados/economia.json`)

Execucao de 2026-11-03 15:37:48, Intel(R) Core(TM) i9-14900HX, 3 repeticoes (mediana), fatia de 8 MiB, medicao principal com 4 trabalhadores em P-cores. Violacoes de bit-exatidao: 0. Celulas com falha: 0.

### Escala multi-nucleo (processos independentes, blocos de 1 MiB)

| N | zstd-3 comp MB/s | ef comp | zstd-3 decomp MB/s | ef decomp | zstd-1 comp MB/s | ef comp |
|---|---|---|---|---|---|---|
| 1 | 500 | 1,08 | 2.049 | 1,06 | 711 | 1,03 |
| 2 | 904 | 1,04 | 3.816 | 1,04 | 1.332 | 0,99 |
| 4 | 1.659 | 0,98 | 6.869 | 0,97 | 2.425 | 0,95 |
| 8 | 3.170 | 0,95 | 13.168 | 0,91 | 4.466 | 0,91 |
| 16 | 4.363 | 0,64 | 19.331 | 0,67 | 6.795 | 0,67 |
| 24 | 5.351 | 0,52 | 19.082 | 0,44 | 8.727 | 0,57 |
| 32 | 5.846 | 0,42 | 14.275 | 0,25 | 10.085 | 0,47 |

Em 32 threads (8P + 16E + 8 irmaos SMT) esta CPU entrega 5.846 MB/s de compressao zstd-3 e 10.085 MB/s de zstd-1, contra um pico exigido de 7.716 MB/s. A queda de eficiencia vem de E-cores (0,54 a 0,76x de um P-core, secao 13 de resumo.md), SMT e queda de clock sob carga, nao de falta de paralelismo; a curva e conservadora para um servidor homogeneo. Atualizacao (Rodada 2, ao final deste arquivo): a escala foi remedida em nucleos homogeneos; so E-cores a 16 da 0,72 (zstd-3) e 0,88 (zstd-1), 8 P fisicos 0,89 e o SMT soma +17%, de modo que ~60% da queda a 16 e a 32 threads e atribuivel a hibrido/SMT e ~40% a limite de sistema (INFERENCIA PROPRIA).

### Nucleos e custo para 5 PB/mes (classe mix_real, blocos de 1 MiB, curva de escala medida)

| config | razao x | comp MB/s | decomp MB/s | N medio | N pico | cabe em 64 | US$/mes nuvem | US$/mes on-prem |
|---|---|---|---|---|---|---|---|---|
| stored | 1,00 | 13.781 | 11.811 | 0,0 | 0,0 | sim | 0 | 0 |
| zstd-1 | 2,35 | 596 | 1.707 | 3,3 | 26,1 | sim | 107 | 32 |
| zlib-6 | 2,54 | 39 | 407 | 118,1 | 472,4 | nao | 3.845 | 1.164 |
| zstd-3 | 2,57 | 378 | 1.611 | 5,3 | 48,4 | sim | 171 | 52 |
| zstd-9 | 2,75 | 98 | 1.736 | 46,8 | 187,1 | nao | 1.523 | 461 |
| zstd-19 | 2,95 | 7 | 1.581 | 661,8 | 2.647 | nao | 21.546 | 6.522 |
| zstd-22 | 2,95 | 6 | 1.538 | 793,7 | 3.175 | nao | 25.842 | 7.822 |
| xz-9e | 3,16 | 5 | 135 | 862,0 | 3.448 | nao | 28.064 | 8.495 |
| bzip2-9 | 3,17 | 22 | 51 | 206,0 | 824,0 | nao | 6.707 | 2.030 |

Cumprem 5 PB/mes com <= 64 nucleos no pico, em todas as classes reais: apenas zstd-1 e zstd-3 (e `stored`). zstd-9 precisa de ~83 nucleos no pico mesmo com a curva so de P-cores; zstd-19, xz-9e e zstd-22 precisam de milhares (sensibilidade completa em `resumo.md`).

### Configuracao otima por classe (cenario nuvem+s3_standard, blocos de 1 MiB)

| classe | otimo irrestrito quente / morno / frio | otimo com N pico <= 64 | razao x | comp MB/s | N pico | US$/mes nuvem |
|---|---|---|---|---|---|---|
| texto | zstd-19 / bzip2-9 / bzip2-9 | zstd-3 | 3,18 | 325 | 56,3 | 200 |
| codigo | zstd-19 / xz-9e / xz-9e | zstd-3 | 3,02 | 517 | 35,4 | 123 |
| binario | zstd-19 / xz-9e / xz-9e | zstd-3 | 1,85 | 411 | 44,5 | 156 |
| cientifico | xz-9e / bzip2-9 / bzip2-9 | zstd-3 | 2,11 | 348 | 52,6 | 186 |
| banco_de_dados | xz-9e / bzip2-9 / bzip2-9 | zstd-3 | 2,85 | 453 | 40,4 | 141 |
| xml | zstd-19 / bzip2-9 / bzip2-9 | zstd-3 | 8,19 | 805 | 12,2 | 78 |
| dna | zstd-19 / zstd-19 / zstd-19 | zstd-3 | 3,31 | 375 | 48,8 | 172 |
| sint_logs | zstd-19 / bzip2-9 / bzip2-9 | zstd-1 | 4,09 | 566 | 32,3 | 112 |
| sint_incompressivel | stored / stored / stored | stored | 1,00 | 14.955 | 0,0 | 0 |
| sint_ja_comprimido | stored / stored / stored | stored | 1,00 | 11.391 | 0,0 | 0 |
| mix_real | zstd-19 / bzip2-9 / bzip2-9 | zstd-3 | 2,57 | 378 | 48,4 | 171 |

O otimo irrestrito e quase sempre a configuracao de maior razao: com S3 Standard a US$0,023/GB-mes e retencao de 60 meses, cada ponto percentual de economia vale ~US$0,014/GB, muito acima do custo de CPU: nas configuracoes otimas o custo total de CPU (compressao mais leituras) e de 0,1% a 6% do valor de armazenamento economizado. O que limita o desenho nao e o US$/GB e sim o requisito de vazao: com <= 64 nucleos no pico o otimo vira zstd-3 (zstd-1 para os logs sinteticos, `stored` para dados incompressiveis ou ja comprimidos). A perda por usar a configuracao restrita esta na coluna 'perda vs irrestrito' de `resumo.md` (US$0,03 a 0,16 por GB no cenario nuvem+s3_standard; US$0,001 a 0,014 por GB no cenario on-prem).

### Penalidade de bloco (aumento do tamanho comprimido vs arquivo inteiro, %)

| classe | modo | zstd-1 | zstd-3 | zstd-19 | xz-9e | zlib-6 | bzip2-9 |
|---|---|---|---|---|---|---|---|
| mix_real | bloco_1MiB | 0,3 | 1,3 | 5,0 | 4,8 | 0,2 | 0,9 |
| mix_real | bloco_64KiB | 4,5 | 10,3 | 17,9 | 18,4 | 3,8 | 14,2 |
| texto | bloco_1MiB | 0,5 | 1,8 | 9,7 | 9,5 | 0,3 | 2,0 |
| texto | bloco_64KiB | 6,2 | 14,0 | 34,3 | 34,4 | 5,7 | 25,5 |
| codigo | bloco_1MiB | 0,7 | 5,4 | 11,9 | 11,8 | 0,3 | 1,4 |
| codigo | bloco_64KiB | 11,8 | 20,9 | 32,3 | 32,3 | 4,7 | 15,8 |
| xml | bloco_1MiB | 1,1 | 2,7 | 7,1 | 6,7 | 0,7 | -0,5 |
| xml | bloco_64KiB | 23,0 | 26,7 | 46,9 | 49,1 | 9,3 | 37,7 |
| binario | bloco_1MiB | 0,3 | 0,8 | 2,2 | 2,2 | 0,1 | -0,2 |
| binario | bloco_64KiB | 1,4 | 6,8 | 9,9 | 10,2 | 1,1 | 4,0 |

Blocos de 1 MiB custam pouco com zstd-1/3 (cerca de 0,3 a 1,3% no mix real) e mais nas configuracoes de janela grande (zstd-19 e xz-9e, ~5%); blocos de 64 KiB custam 4 a 18% no mix real e ate ~50% em XML nas configuracoes fortes. Quanto mais forte a configuracao (janela maior, mais busca), maior a penalidade do bloco pequeno.

### Ganho de dicionario treinado (zstd, blocos de 64 KiB, dicionario ~110 KB)

| classe | config | razao sem | razao com | ganho de razao % | reducao de tamanho % | comp MB/s sem -> com | decomp MB/s sem -> com |
|---|---|---|---|---|---|---|---|
| codigo | zstd-3 | 4,57 | 4,67 | 2,1 | 2,1 | 568 -> 373 | 1.427 -> 1.217 |
| codigo | zstd-9 | 5,12 | 5,33 | 4,0 | 3,9 | 118 -> 90 | 1.531 -> 1.247 |
| sint_logs | zstd-3 | 3,90 | 3,76 | -3,7 | -3,8 | 501 -> 285 | 1.613 -> 1.003 |
| sint_logs | zstd-9 | 4,24 | 4,29 | 1,2 | 1,1 | 88 -> 60 | 1.388 -> 1.039 |
| texto | zstd-3 | 2,85 | 3,06 | 7,7 | 7,1 | 310 -> 236 | 1.272 -> 862 |
| texto | zstd-9 | 3,07 | 3,40 | 10,5 | 9,5 | 69 -> 44 | 1.380 -> 916 |
| xml | zstd-3 | 6,97 | 6,94 | -0,4 | -0,4 | 717 -> 475 | 1.757 -> 1.852 |
| xml | zstd-9 | 8,05 | 8,10 | 0,7 | 0,7 | 148 -> 128 | 2.567 -> 1.974 |

O dicionario ajuda texto (+8 a +11% de razao) e um pouco codigo (+2 a +4%); nao ajuda XML (-0,4 a +0,7%) nem os logs sinteticos (-3,7 a +1,2%; campos aleatorios dominam) e custa 14 a 43% da vazao de compressao e ate 38% da de descompressao em blocos de 64 KiB (a cada quadro as tabelas do dicionario sao copiadas para o contexto). Treino e teste do mesmo arquivo: ganho provavelmente otimista.

### Recompressao em camadas: M* (meses de vida restante para o reempacotamento se pagar), origem zstd-3, blocos de 1 MiB

| classe | destino | nuvem+s3_standard (quente/morno/frio) | nuvem+b2 | nuvem+glacier_ir | onprem+onprem |
|---|---|---|---|---|---|
| mix_real | zstd-19 | 3,7 / 3,7 / 3,7 | 12,3 / 12,3 / 12,3 | 21,5 / 21,4 / 21,4 | 13,0 / 12,9 / 12,9 |
| mix_real | xz-9e | 5,0 / 3,4 / 3,3 | nunca / 11,7 / 11,0 | nunca / 21,2 / 19,2 | nunca / 12,3 / 11,6 |
| texto | zstd-19 | 3,7 / 3,7 / 3,7 | 12,2 / 12,3 / 12,3 | 21,1 / 21,3 / 21,3 | 12,8 / 12,9 / 12,9 |
| texto | xz-9e | 5,5 / 4,1 / 4,1 | 100,4 / 14,2 / 13,6 | nunca / 25,4 / 23,6 | 158,2 / 14,9 / 14,3 |
| codigo | zstd-19 | 5,1 / 5,1 / 5,1 | 17,0 / 16,9 / 16,9 | 29,6 / 29,3 / 29,3 | 17,8 / 17,8 / 17,8 |
| codigo | xz-9e | 14,5 / 6,0 / 5,8 | nunca / 21,4 / 19,3 | nunca / 40,4 / 33,6 | nunca / 22,6 / 20,3 |

Regra pratica: com S3 Standard, reempacotar zstd-3 para zstd-19 (ganho de 5,0 pontos percentuais de economia no mix real, custo de reempacotar US$0,0043/GB) se paga em ~3,7 meses de vida restante, em qualquer camada de leitura, porque a descompressao de zstd-19 e tao rapida quanto a de zstd-3. Com B2 e on-prem o ponto de equilibrio sobe para ~12 a 13 meses e com Glacier IR para ~21 meses no mix real (codigo: 17 a 30 meses). Para xz-9e o equilibrio depende da camada porque a leitura custa mais (xz descomprime a ~135 MB/s): nas camadas mornas e frias e parecido com o de zstd-19, na camada quente (n=100) com armazenamento barato praticamente nunca se paga (nunca no mix real com B2, Glacier IR e on-prem). O limite de leituras tolerado por vida restante esta em `economia.json` (`n_max_leituras_na_vida_restante`).

### P-core vs E-core, memoria e conferencia cruzada

E-core / P-core (1 nucleo isolado, dickens+mozilla): 0,54 a 0,76 (mediana 0,61).

Vies da medicao principal (4 trabalhadores concorrentes) na compressao em blocos de 1 MiB: 0,89 a 0,94 da vazao do P-core isolado (a medicao principal e ligeiramente pessimista).

| config | RSS compressao MB | RSS descompressao MB |
|---|---|---|
| zstd-3 | 43,5 | 6,8 |
| zstd-9 | 54,8 | 9,4 |
| zstd-19 | 130,7 | 13,5 |
| zstd-22 | 441,9 | 37,8 |
| zstd-3-long27 | 42,7 | 37,8 |
| xz-9e | 363,2 | 35,1 |

RSS medido com entrada de 32 MiB; zstd-22 e xz-9e crescem com a janela e com entradas maiores.

### Conferencia cruzada do harness com `zstd -b` (CLI 1.5.7, mesma fatia, CPU 0)

Compressao do harness (Python em processo) e 5 a 13% mais rapida que a do CLI (hipotese: o CLI liga o checksum de conteudo por padrao; os tamanhos comprimidos diferem em menos de 0,2%). Descompressao do harness e mais lenta: 54 a 64% do CLI em arquivo inteiro (hipotese: cada chamada aloca e toca uma saida nova de 8 MiB, enquanto o CLI reutiliza buffers) e 76 a 83% em blocos de 1 MiB. O modelo, portanto, subestima a vazao de leitura em ~17 a 24% nos blocos de 1 MiB (custo de leitura superestimado; efeito pequeno no resultado liquido, relevante so na camada quente). Tabela completa na secao 14 de `resumo.md`.

### Incompressivel e ja comprimido

`sint_incompressivel` e `sint_ja_comprimido` ficam em razao 1,00 em todas as configuracoes (bzip2-9 em blocos de 64 KiB expande 1%); o modelo escolhe `stored` (liquido 0). zstd-1/3 ainda rodam a 5 a 6 GB/s nesses dados (blocos de 1 MiB), entao uma regra de deteccao barata (tentar zstd-1 e desistir) custa pouco CPU.

### Efeito de `--long=27`

Em fatias de 8 MiB nao muda nada (a janela de 2^21 ja cobre a fatia). Em arquivos inteiros maiores (secao `completo` de `medicoes.json`), a razao de zstd-3 sobe de 2,81 para 2,84 em mozilla, de 4,36 para 4,47 em samba, de 11,84 para 14,18 em nci e de 3,43 para 3,45 em webster, ao custo de 17 a 62% da vazao de compressao.

### Falhas, cortes e incertezas desta execucao

- Primeira execucao descartada: a fase principal teve 58 celulas com erro (zstd-3-long27 em arquivo inteiro: `compression.zstd` rejeita `level` junto de `options`; e o complemento de arquivo completo sem o campo `config`). Corrigi o codigo e rodei de novo TUDO de ponta a ponta (`--fase tudo`, ~9 min de relogio); `resultados/` vem so da segunda execucao, sem falhas e sem violacoes de bit-exatidao.
- Cortes por tempo: fatia de 8 MiB por arquivo; modos em bloco so com um subconjunto de configuracoes (sem xz-1, xz-6, zlib-1, zlib-9 e zstd-3-long27; sem zstd-22 em 64 KiB); dicionario so com zstd-3 e zstd-9; escala multi-nucleo so com zstd-3 e zstd-1 (rodadas de 1,5 s); P-core vs E-core so em dickens e mozilla.
- Medicao principal com 4 trabalhadores concorrentes: vazao de compressao 6 a 11% abaixo do P-core isolado em blocos de 1 MiB; ate 23% abaixo em arquivo inteiro com configuracoes lentas (zstd-19, xz-6).
- Energia nao medida (RAPL sem permissao).
- Incertezas dominantes: (1) curva de eficiencia de escala (a de 32 threads mistura E-cores e SMT e e conservadora para servidor homogeneo; a de so P-cores ate N=8 e otimista) e o fator servidor/notebook (1,0); (2) precos on-prem e do armazenamento on-prem sao INFERENCIA PROPRIA e os de nuvem nao foram reverificados; (3) razoes de fatias de 8 MiB e classes sinteticas; (4) dicionario treinado e testado na mesma fonte; (5) sem dedup/CDC e sem compressao neural.

## Rodada 2: gate F1 (dedup por bloco) e escala em nucleos homogeneos (execucao de 2026-11-03)

Numeros brutos em `resultados/dedup.json` (e tabelas completas em `resultados/dedup.md`), `resultados/escala_homogenea.json` e `resultados/escala_homogenea.md`. `medicoes.json` nao foi alterado. Violacoes de bit-exatidao: 0 (em todas as 43 tarefas de analise, nos 3 baselines de fluxo, no `--long=31` e em todas as medicoes de escala). Tempos de relogio das fases: analise 159 s (16 processos), delta 1 s, `--long=31` 21 s, vazao ~30 s (1 nucleo), escala homogenea 327 s.

### Testes de sanidade do chunker (TODOS APROVADOS)

| chunker | media / alvo nominal (256 MiB aleatorios) | chunks do original que reaparecem apos inserir 100 B no inicio (64 MiB) | round-trip | determinismo |
|---|---|---|---|---|
| fixo 4K, 8K, 64K, 1M | 1,000 | 0% (0/16384, 0/8192, 0/1024, 0/64) | ok | ok |
| CDC 8K | 1,142 | 99,9% (7165/7172) | ok | ok |
| CDC 64K | 1,148 | 99,9% (894/895) | ok | ok |
| CDC 1M | 1,169 | 98,1% (53/54) | ok | ok |

(a) "Media ~ alvo": os tamanhos medios do CDC ficam 14 a 17% ACIMA do nominal. Nao e defeito: com `min = medio/4`, normalizacao nivel 2 e `max = medio*8` a media teorica e ~1,14x o nominal (min + cauda das duas mascaras), e a medida bate (1,142 a 1,169). Todas as tabelas usam o tamanho nominal como rotulo e o tamanho medio observado de cada conjunto esta em `media_chunk_unico` de `dedup.json`. Os chunks respeitam min e max. (b) A insercao de 100 B quebra 1 a 7 chunks do CDC e todos os de tamanho fixo. (c) Offsets contiguos, concatenacao e sha256 iguais ao original.

### Gate F1: ganho extra do CDC sobre dedup de arquivo inteiro + zstd-3

Baseline da arvore = arquivo inteiro + zstd-3; baseline do fluxo (tar e sintetico, onde nao existe dedup de arquivo) = zstd-3 do fluxo inteiro sem dedup. Indice = chunks unicos x 32 B; extrapolacao para 5 PB/mes e INFERENCIA PROPRIA (formula na metodologia); "meses em 1 TB / 4 TB" supoe indice residente em RAM e a mesma taxa de dedup do par para todo o dado ingerido.

| conjunto | estrategia | % de v2 ja em v1 | razao total zstd-3 | ganho extra sobre o baseline | atende 5% | indice 32 B | indice TB/mes a 5 PB/mes | meses de ingestao em 1 TB / 4 TB de RAM |
|---|---|---|---|---|---|---|---|---|
| A_arvore | arquivo inteiro + zstd-3 (BASELINE) | 99.17% | 10.42 | 0 | - | 2.8 MB | - | - |
| A_arvore | fixo_4K | 99.50% | 7.63 | -36.5% | nao | 13.17 MB | 22.24 | 0.04 / 0.18 |
| A_arvore | fixo_8K | 99.47% | 8.48 | -22.9% | nao | 7.56 MB | 12.76 | 0.08 / 0.31 |
| A_arvore | fixo_64K | 99.28% | 9.76 | -6.7% | nao | 3.11 MB | 5.25 | 0.19 / 0.76 |
| A_arvore | fixo_1M | 99.17% | 10.29 | -1.2% | nao | 2.77 MB | 4.68 | 0.21 / 0.85 |
| A_arvore | cdc_8K | 99.79% | 9.05 | -15.0% | nao | 6.19 MB | 10.45 | 0.10 / 0.38 |
| A_arvore | cdc_64K | 99.37% | 9.94 | -4.8% | nao | 3.01 MB | 5.09 | 0.20 / 0.79 |
| A_arvore | cdc_1M | 99.17% | 10.34 | -0.7% | nao | 2.77 MB | 4.68 | 0.21 / 0.86 |
| A_tar | zstd-3 do fluxo, sem dedup (BASELINE) | 0 | 6.96 | 0 | - | - | - | - |
| A_tar | fixo_4K | 45.00% | 4.72 | -47.5% | nao | 18.76 MB | 30.25 | 0.03 / 0.13 |
| A_tar | fixo_8K | 11.36% | 4.82 | -44.5% | nao | 11.42 MB | 18.42 | 0.05 / 0.22 |
| A_tar | fixo_64K | 10.41% | 5.97 | -16.7% | nao | 1.44 MB | 2.31 | 0.43 / 1.73 |
| A_tar | fixo_1M | 10.21% | 7.12 | +2.3% | nao | 0.09 MB | 0.14 | 6.90 / 27.62 |
| A_tar | cdc_8K | 99.58% | 9.56 | +27.2% | SIM | 4.47 MB | 7.20 | 0.14 / 0.56 |
| A_tar | cdc_64K | 98.38% | 11.25 | +38.2% | SIM | 0.59 MB | 0.96 | 1.04 / 4.17 |
| A_tar | cdc_1M | 89.66% | 11.72 | +40.6% | SIM | 0.04 MB | 0.06 | 15.54 / 62.17 |
| B_arvore | arquivo inteiro + zstd-3 (BASELINE) | 66.23% | 7.30 | 0 | - | 3.4 MB | - | - |
| B_arvore | fixo_4K | 74.80% | 5.81 | -25.7% | nao | 16.36 MB | 27.61 | 0.04 / 0.14 |
| B_arvore | fixo_8K | 73.31% | 6.35 | -14.9% | nao | 9.42 MB | 15.90 | 0.06 / 0.25 |
| B_arvore | fixo_64K | 67.32% | 6.96 | -4.8% | nao | 3.83 MB | 6.46 | 0.15 / 0.62 |
| B_arvore | fixo_1M | 66.23% | 7.23 | -0.9% | nao | 3.42 MB | 5.77 | 0.17 / 0.69 |
| B_arvore | cdc_8K | 81.10% | 7.16 | -1.9% | nao | 7.37 MB | 12.44 | 0.08 / 0.32 |
| B_arvore | cdc_64K | 69.50% | 7.13 | -2.4% | nao | 3.70 MB | 6.24 | 0.16 / 0.64 |
| B_arvore | cdc_1M | 67.21% | 7.27 | -0.4% | nao | 3.41 MB | 5.76 | 0.17 / 0.69 |
| B_tar | zstd-3 do fluxo, sem dedup (BASELINE) | 0 | 6.97 | 0 | - | - | - | - |
| B_tar | fixo_4K | 15.35% | 4.22 | -65.1% | nao | 22.37 MB | 36.04 | 0.03 / 0.11 |
| B_tar | fixo_8K | 11.81% | 4.65 | -49.9% | nao | 11.40 MB | 18.37 | 0.05 / 0.22 |
| B_tar | fixo_64K | 0.11% | 5.60 | -24.4% | nao | 1.51 MB | 2.44 | 0.41 / 1.64 |
| B_tar | fixo_1M | 0.00% | 6.68 | -4.3% | nao | 0.09 MB | 0.15 | 6.55 / 26.21 |
| B_tar | cdc_8K | 71.74% | 6.89 | -1.1% | nao | 5.84 MB | 9.40 | 0.11 / 0.43 |
| B_tar | cdc_64K | 48.04% | 6.79 | -2.5% | nao | 0.91 MB | 1.46 | 0.68 / 2.73 |
| B_tar | cdc_1M | 31.36% | 7.11 | +2.1% | nao | 0.06 MB | 0.10 | 9.87 / 39.46 |
| sintetico | zstd-3 do fluxo, sem dedup (BASELINE) | 0 | 3.11 | 0 | - | - | - | - |
| sintetico | fixo_4K | 0.93% | 2.33 | -33.4% | nao | 3.31 MB | 38.84 | 0.03 / 0.10 |
| sintetico | fixo_8K | 0.77% | 2.49 | -24.7% | nao | 1.66 MB | 19.46 | 0.05 / 0.21 |
| sintetico | fixo_64K | 0.70% | 2.83 | -9.9% | nao | 0.21 MB | 2.43 | 0.41 / 1.64 |
| sintetico | fixo_1M | 0.00% | 3.07 | -1.3% | nao | 0.01 MB | 0.15 | 6.53 / 26.11 |
| sintetico | cdc_8K | 92.58% | 4.63 | +32.8% | SIM | 0.82 MB | 9.65 | 0.10 / 0.41 |
| sintetico | cdc_64K | 68.97% | 4.31 | +27.8% | SIM | 0.12 MB | 1.40 | 0.72 / 2.86 |
| sintetico | cdc_1M | 0.37% | 3.08 | -0.9% | nao | 0.01 MB | 0.13 | 7.61 / 30.43 |

Leitura (as conclusoes marcadas INFERENCIA PROPRIA vao alem do medido):

- ARVORE (Par A e Par B): nenhum chunker atinge 5%; o melhor e CDC 1M com -0,7% (A) e -0,4% (B). O dedup de arquivo inteiro ja encontra 99,17% de v2 em v1 no Par A e 66,2% no Par B; o CDC 8K acha mais (99,79% e 81,1%, ou seja +0,6 e +14,9 pontos), mas cada chunk comprimido sozinho com zstd-3 perde mais que o dedup extra ganha: a razao de compressao do conteudo unico e 5,24 por arquivo inteiro (Par A; 4,87 no Par B) contra 4,07 (3,89) com CDC 8K, 4,81 (4,51) com CDC 64K e 5,19 (4,81) com CDC 1M, e o CDC 1M, que quase nao perde compressao, tambem quase nao ganha dedup (chunk = arquivo). Pelo criterio F1 literal ("ganho extra < 5%") o CDC deve ser ABANDONADO para dados deste tipo (codigo-fonte em muitos arquivos pequenos, arquivo medio de 17 KiB) quando o motor enxerga os arquivos.
- FLUXO TAR, Par A (99% de v2 igual a v1, so 223 arquivos mudaram): CDC atende o limite com folga (8K +27,2%, 64K +38,2%, 1M +40,6%), mas o baseline aqui nao tem dedup nenhum; tamanho fixo nao atende (4K a 64K negativo; 1M +2,3%, abaixo de 5%) porque o deslocamento dos membros do tar desalinha os blocos (4K acha so 45% de v2 em v1; 8K e 64K, ~10%).
- FLUXO TAR, Par B (similaridade menor): CDC NAO atende (1M +2,1%; 8K -1,1%; 64K -2,5%).
- SINTETICO: CDC 8K (+32,8%) e 64K (+27,8%) atendem; CDC 1M nao (-0,9%) porque 835 edicoes de 1 a 4 KiB caem em quase todos os ~170 chunks de 1 MiB (0,37% de v2 reaparece); tamanho fixo e negativo em todas as medidas (1% de insercoes desloca tudo).
- Comparacao entre representacoes (INFERENCIA PROPRIA, nao pedida no gate, mas muda a leitura): em bytes armazenados, CDC 1M sobre o tar do Par A (264,6 MB) fica 7% abaixo do dedup de arquivo inteiro sobre a arvore (284,4 MB), e no Par B fica 7,5% acima (436,4 contra 406,0 MB). Boa parte do ganho do CDC 1M sobre o tar no Par A vem de comprimir blocos grandes (zstd-3 do tar inteiro da razao 6,96; zstd-3 por arquivo da 5,26 antes de qualquer dedup), nao de dedup.
- `--long=31` (EXTRA, sem indice nenhum, sem acesso aleatorio): zstd-3 com janela de 2 GiB sobre v1+v2 concatenados da razao 14,98 (Par A), 14,56 (Par B) e 6,11 (sintetico), acima de qualquer CDC+zstd-3 por chunk (11,72; 7,11; 4,63). O chunk comprimido sozinho joga fora o contexto entre chunks; isso e custo estrutural do desenho "chunk unico independente".
- INDICE: com 1 TB de RAM, CDC 8K nao cabe em nenhum conjunto (7,2 a 12,4 TB de indice por mes ingerido; ~0,1 mes), CDC 64K cabe ~1 mes no fluxo (0,96 a 1,46 TB/mes) e ~0,2 mes na arvore (5,1 a 6,2 TB/mes), e CDC 1M cabe 7,6 a 15,5 meses no fluxo, mas na arvore o chunk de 1 MiB equivale ao arquivo (media 17 KiB) e o indice (4,7 a 5,8 TB/mes) tambem nao cabe. Com 4 TB os numeros quadruplicam (tabela). Pior caso sem nenhuma dedup (todo chunk unico): CDC 8K 15,8 a 22,5 TB/mes, CDC 64K 1,95 (fluxo) a 10,3 (arvore), CDC 1M 0,12 (fluxo) a 9,4 (arvore), em `pior_caso_sem_dedup_TB_por_mes` de `dedup.json`. O segundo criterio de F1 (indice nao cabe na RAM prevista) tambem reprova CDC 8K e 64K nas premissas de 1 TB e retencao acima de 1 mes.

Resumo do gate: Par A arvore: abandonar (-0,7% a -15%); Par B arvore: abandonar (-0,4% a -26%); Par A tar: CDC atende o limite mas so contra um baseline sem dedup, e so 64K/1M tem indice viavel em 1 TB (por ~1 e ~15 meses); Par B tar: abandonar (+2,1% no melhor caso); sintetico: CDC 8K/64K atendem, indice de 8K nao cabe.

### zstd-19 e referencia sem indice

zstd-19 e ESTIMATIVA por amostra de 1/4 dos chunks unicos (ver metodologia); so CDC 64K e 1M.



### Delta por caminho (limite superior de dedup por similaridade)

Custo marginal = bytes comprimidos para guardar os arquivos alterados de v2 (mesmo caminho, conteudo diferente) quando v1 ja esta guardado.



Par A: 223 arquivos alterados (12,3 MB logicos) custam 42,6 KB com delta por caminho contra 1,04 MB (CDC 8K), 2,62 MB (64K) e 3,33 MB (1M, igual ao zstd-3 sozinho: os chunks de 1 MiB nao reaparecem); no total de ~284 MB isso vale 1,2% (281,1 contra 284,4 MB). Par B: 19.227 arquivos alterados (493 MB logicos, zstd-3 sozinho 123,1 MB) custam 3,40 MB com delta por caminho contra 85 a 122 MB com CDC (o CDC recupera so 0,5% (1M), 4% (64K) e 32% (8K) do que o delta recupera): o total do delta (286,5 MB, razao 10,34) fica 29% abaixo do arquivo inteiro (406,0 MB) e 31% abaixo do CDC 64K (415,8 MB). INFERENCIA PROPRIA: em dados de codigo, a similaridade entre arquivos de mesmo caminho (edicoes pequenas em arquivos de ~25 KiB) e muito maior que a que o CDC captura com chunks de 8 KiB ou mais; se o gate F1 for "abandonar CDC", delta/similaridade e a alternativa a investigar. Conferencia do prefixo `compression.zstd` (com long-distance matching, que o `--patch-from` do CLI liga) contra o CLI em 40 arquivos por par: soma de bytes 9.341 contra 9.453 (A) e 4.067 contra 4.232 (B). Sem o long-distance matching o prefixo dava 5% (A) e 52% (B) a mais que o CLI nessa mesma amostra; essa primeira versao foi descartada e a fase `delta` foi refeita.

### Vazao em 1 nucleo (CPU 0, P-core fisico; mediana de 3; fatia de 256 MiB; MB = 1e6 B)



blake2b em um unico buffer de 256 MiB: 1.247 MB/s (BLAKE3 NAO medido). O chunker FastCDC roda a ~3,4 GB/s (3.400 a 3.470 MB/s, quase independente do tamanho medio); o hash e o gargalo (1,16 a 1,25 GB/s por chunk); chunker+hash+lookup fica em 0,88 a 0,92 GB/s para CDC e 1,12 a 1,25 GB/s para tamanho fixo (o lookup em `dict` custa pouco aqui porque o dicionario tem ate ~27 mil entradas e cabe na cache; um indice com 10^11 entradas em RAM/NVMe custa uma falha de cache ou I/O por consulta e nao esta representado). INFERENCIA PROPRIA: a ~0,9 GB/s por nucleo, 5 PB/mes (media de 1,93 GB/s) pedem ~2,2 nucleos so para chunk+hash+lookup (~8,6 no pico de 7,7 GB/s com k=4), antes de compressao e de I/O do indice.

### Escala em nucleos homogeneos (zstd-3 e zstd-1, blocos de 1 MiB)

Vazao isolada (1 nucleo, pool inteiro de blocos):

| config | P-core isolado comp / decomp MB/s | E-core isolado comp / decomp MB/s | E/P comp | E/P decomp |
|---|---|---|---|---|
| zstd-3_bloco_1MiB | 463 / 1904 | 275 / 1205 | 0.593 | 0.633 |
| zstd-1_bloco_1MiB | 665 / 1967 | 411 / 1300 | 0.618 | 0.661 |

Razao E-core/P-core de vazao isolada: 0,59 (zstd-3 compressao), 0,63 (zstd-3 descompressao), 0,62 e 0,66 (zstd-1) (a rodada anterior mediu 0,54 a 0,76, mediana 0,61, em outro subconjunto).

zstd-3, compressao e descompressao (agregado em MB/s; "ef vs mesmo tipo" = relativa a 1 nucleo do mesmo tipo, "ef vs P-core" = definicao da curva anterior):

| grupo | N | comp agregado MB/s | comp por nucleo MB/s | ef comp vs mesmo tipo | ef comp vs P-core | decomp agregado MB/s | ef decomp vs mesmo tipo | ef decomp vs P-core |
|---|---|---|---|---|---|---|---|---|
| (a) so E-cores fisicos | 1 | 299 | 299 | 1.01 | 0.60 | 1290 | 1.01 | 0.62 |
| (a) so E-cores fisicos | 2 | 536 | 268 | 0.97 | 0.57 | 2285 | 0.93 | 0.58 |
| (a) so E-cores fisicos | 4 | 806 | 202 | 0.74 | 0.44 | 4007 | 0.84 | 0.53 |
| (a) so E-cores fisicos | 8 | 1627 | 203 | 0.76 | 0.45 | 7862 | 0.83 | 0.52 |
| (a) so E-cores fisicos | 12 | 2386 | 199 | 0.73 | 0.43 | 11542 | 0.80 | 0.50 |
| (a) so E-cores fisicos | 16 | 3128 | 195 | 0.72 | 0.43 | 13890 | 0.72 | 0.46 |
| (b) so P-cores fisicos | 1 | 500 | 500 | 1.00 | 1.00 | 2071 | 1.00 | 1.00 |
| (b) so P-cores fisicos | 2 | 912 | 456 | 0.98 | 0.98 | 3843 | 0.98 | 0.98 |
| (b) so P-cores fisicos | 4 | 1651 | 413 | 0.91 | 0.91 | 6869 | 0.90 | 0.90 |
| (b) so P-cores fisicos | 8 | 3196 | 399 | 0.89 | 0.89 | 13257 | 0.87 | 0.87 |
| (c) P-cores com SMT | 8 | 3122 | 390 | 0.87 | 0.87 | 13052 | 0.86 | 0.86 |
| (c) P-cores com SMT | 16 | 3729 | 233 | 0.51 | 0.51 | 14639 | 0.48 | 0.48 |
| (d) 8 P + 8 E | 16 | 4310 | 269 | 0.74 | 0.59 | 19195 | 0.77 | 0.63 |

zstd-1:

| grupo | N | comp agregado MB/s | comp por nucleo MB/s | ef comp vs mesmo tipo | ef comp vs P-core | decomp agregado MB/s | ef decomp vs mesmo tipo | ef decomp vs P-core |
|---|---|---|---|---|---|---|---|---|
| (a) so E-cores fisicos | 1 | 436 | 436 | 1.00 | 0.61 | 1412 | 1.04 | 0.68 |
| (a) so E-cores fisicos | 2 | 819 | 410 | 0.98 | 0.60 | 2573 | 0.97 | 0.65 |
| (a) so E-cores fisicos | 4 | 1418 | 355 | 0.89 | 0.55 | 4500 | 0.87 | 0.58 |
| (a) so E-cores fisicos | 8 | 2767 | 346 | 0.90 | 0.56 | 8828 | 0.87 | 0.57 |
| (a) so E-cores fisicos | 12 | 4164 | 347 | 0.89 | 0.55 | 12944 | 0.84 | 0.55 |
| (a) so E-cores fisicos | 16 | 5536 | 346 | 0.88 | 0.54 | 15807 | 0.77 | 0.51 |
| (b) so P-cores fisicos | 1 | 727 | 727 | 1.02 | 1.02 | 2125 | 1.02 | 1.02 |
| (b) so P-cores fisicos | 2 | 1360 | 680 | 0.99 | 0.99 | 4012 | 1.01 | 1.01 |
| (b) so P-cores fisicos | 4 | 2414 | 603 | 0.93 | 0.93 | 7151 | 0.92 | 0.92 |
| (b) so P-cores fisicos | 8 | 4544 | 568 | 0.91 | 0.91 | 13774 | 0.89 | 0.89 |
| (c) P-cores com SMT | 8 | 4452 | 556 | 0.89 | 0.89 | 13604 | 0.88 | 0.88 |
| (c) P-cores com SMT | 16 | 5542 | 346 | 0.54 | 0.54 | 16395 | 0.53 | 0.53 |
| (d) 8 P + 8 E | 16 | 6769 | 423 | 0.83 | 0.66 | 20619 | 0.79 | 0.66 |

Conclusao (aritmetica sobre os numeros acima; a atribuicao multiplicativa e INFERENCIA PROPRIA, nao foram feitos experimentos que isolem cada efeito):

1. 16 E-cores homogeneos: eficiencia 0,72 (zstd-3, compressao e descompressao) e 0,88 / 0,77 (zstd-1, compressao / descompressao). Nao chega a 0,9 em zstd-3, entao existe um limite que nao e "so o fato de a CPU ser hibrida"; tambem esta bem acima de ~0,6. O resultado cai no meio, e depende do codec.
2. Onde esta esse limite: na compressao nos E-cores a queda ocorre entre N=2 (0,97) e N=4 (0,74) e depois fica plana (0,76, 0,73, 0,72 em N=8, 12, 16). Em `/sys/devices/system/cpu/cpu*/cache/index2/shared_cpu_list` os E-cores aparecem em clusters de 4 (16-19, 20-23, 24-27, 28-31) dividindo 4 MiB de L2 (1 MiB por E-core; os P-cores tem 2 MiB privados por nucleo fisico). O ponto em que a eficiencia cai coincide com o preenchimento de um cluster e ela nao piora ao somar clusters (1 a 4), o que aponta para a L2 compartilhada do cluster (blocos de 1 MiB e zstd-3 tem working set grande; zstd-1 usa menos e cai menos) e nao para banda de memoria ou L3 globais. Na descompressao (13,9 GB/s de saida com 16 E-cores) a eficiencia cai mais gradualmente (0,93; 0,84; 0,83; 0,80; 0,72 em N=2, 4, 8, 12, 16), compativel com uma componente global adicional (L3/memoria) alem da L2 de cluster. O efeito de cluster e uma caracteristica do desenho hibrido: um servidor com L2 privada por nucleo nao teria essa parte.
3. P-cores fisicos (reconfirmacao): zstd-3 compressao 0,98 / 0,91 / 0,89 em N=2 / 4 / 8 (descompressao 0,98 / 0,90 / 0,87), queda gradual (clock com varios nucleos ativos, L3, potencia). Nao ha como medir acima de 8 P-cores nesta maquina. Repetibilidade: N=8 em 8 P fisicos medido duas vezes (grupos P e PSMT): 3.196 contra 3.122 MB/s (2,3%).
4. SMT: 16 threads em 8 P-cores entregam 1,17x (compressao) e 1,10x (descompressao) de 8 threads em 8 fisicos no zstd-3 (3.196 para 3.729 MB/s e 13.257 para 14.639), e 1,22x / 1,19x no zstd-1. As 16 threads valem ~9,3 nucleos fisicos carregados (cada thread rende ~58% do que renderia com o nucleo so para ela e 47% do P-core isolado); contar a thread SMT como um nucleo cheio superestima a capacidade em ~70%.
5. Misto 8 P + 8 E (N=16, as mesmas CPUs da curva anterior): 4.310 MB/s de compressao zstd-3 (a curva anterior deu 4.363), eficiencia vs P-core 0,59 nesta rodada. A curva anterior reportou 0,64 porque a referencia isolada daquela rodada era ~8% pessimista (a sua eficiencia em N=1 foi 1,08; 0,64/1,08 = 0,59, igual a de agora). Contra o mesmo tipo de nucleo a eficiencia do misto e 0,74 (compressao) e 0,77 (descompressao).
6. Atribuicao da queda no N=16 (compressao zstd-3, fator multiplicativo, INFERENCIA PROPRIA): E-core 0,593x do P-core: x0,80; L2 compartilhada do cluster de E (peso dos E-cores no ideal, eficiencia 0,76 de N=8): x0,91; 8 P-cores em conjunto (0,89): x0,93; resto, P e E ativos ao mesmo tempo (potencia do pacote, L3, anel): x0,87. Produto: 0,59. Em escala logaritmica ~60% da queda e atribuivel a hibrido (E lento e L2 de cluster) e ~40% a limite de sistema (clock/potencia/L3).
7. N=32 (curva anterior, 0,42; NAO foi remedido nesta rodada, entao e INFERENCIA misturando rodadas): 0,42 ~ 0,80 (E mais lento) x 0,74 (SMT: as 8 threads irmas contam como nucleos cheios no ideal, mas somam ~17%) x 0,71 (contencao e potencia restantes, 0,66 se corrigir o vies de 8% da referencia); ~61% da queda e hibrido/SMT, ~39% sistema.
8. Para o modelo de nucleos (INFERENCIA PROPRIA): um servidor homogeneo de P-cores equivalentes deveria escalar proximo da parte "sistema" da decomposicao (~0,8 a 0,9 em 8 a 16 nucleos nesta classe de clock e de cache), nao a 0,64 (16) nem a 0,42 (32) da curva anterior; acima de 16 nucleos nao ha dado homogeneo aqui. Isso nao muda a conclusao de que zstd-1 e zstd-3 cabem em 64 nucleos no pico; muda a margem.

### Falhas, cortes e incertezas da rodada 2

- Quota de tmpfs: a primeira execucao de `preparar` (3 extracoes em paralelo) terminou com `EDQUOT` (`/tmp` montado com `usrquota`; a folga real do usuario era ~7,5 GiB apesar do `df` mostrar 11 GB livres) e ate impediu o proprio Bash de rodar; um subagente apagou so as extracoes parciais que eu tinha criado. Refiz o preparo sequencial, gerando o tar e APAGANDO cada arvore em seguida (ver metodologia); a arvore e lida do tar por offset. Isso e um desvio da letra do pedido ("arvore extraida") e esta registrado nos dados (`offsets_no_tar_conferem_com_sha256_dos_arquivos`).
- Executei a analise duas vezes por engano (o `ps` do ambiente nao mostrava a primeira); matei a segunda antes de terminar e `dedup.json` vem da primeira, completa, sem violacoes. A fase `delta` foi refeita depois da conferencia com o CLI (ver Delta); a `analise` nao dependia dela.
- zstd-19 so como estimativa por amostra de 1/4 dos chunks unicos (corte por tempo); nao ha baseline zstd-19 de arquivo inteiro; o gate usa so zstd-3.
- O comparador do gate para fluxo e "zstd-3 sem dedup", nao dedup de arquivo inteiro (inexistente no fluxo); a comparacao entre representacoes acima e INFERENCIA PROPRIA e os logicos diferem (o tar inclui cabecalhos e padding de 512 B).
- Dados: so codigo-fonte do kernel (muitos arquivos pequenos, texto) e um par SINTETICO sobre o Silesia; nao representam VM, banco de dados nem backups reais. As conclusoes de F1 nao valem para esses; o par B e "minor" entre as ultimas versoes de duas series, nao entre releases adjacentes no tempo.
- Extrapolacao de indice assume a taxa de dedup observada em UM par de versoes para 5 PB/mes e indice 100% em RAM; metadados e receitas fora do indice de 32 B nao entram no indice.
- Hash blake2b no lugar de BLAKE3; `dict` do Python no lugar de indice de producao; vazao medida em fatia de 256 MiB de um unico arquivo, 1 nucleo, sem I/O.
- Escala: mesma rodada de 1,5 s x 3 por ponto, 13 pontos por codec; a referencia isolada tem ruido de ~8% entre rodadas (ver item 5); N=32 e a decomposicao do N=32 nao foram remedidos; a atribuicao entre L2 de cluster e potencia/L3 e inferencia sem experimento de controle (nao testei, por exemplo, E-cores espalhados um por cluster, que separaria L2 compartilhada de limite global); energia continua nao medida.
- Tempo: o orcamento de 60 min de relogio provavelmente foi excedido no total (pesquisa, codigo e uma falha de quota); as fases de medicao em si somam ~11 min.


## Rodada 3: classificador barato de plano por bloco (componente B), `plano_bloco.py`, execucao de 2026-11-04

Pergunta: vale a pena um classificador de microssegundos que preve, por bloco, o melhor plano (transform + codec + nivel) para gravar no envelope, em vez de medir todos os candidatos? Criterio de parada (declarado antes de rodar): o componente B so vale se, em leave-one-file-out e globalmente, (i) regret de bytes <= 1% do oraculo de tamanho do caminho sincrono, E (ii) ganho >= 1% de bytes sobre `probe+zstd-3` no corpus misto, E (iii) custo de features+inferencia <= 2% do tempo do zstd-3; e se `probe+zstd-3` ja fica a <= 1% do oraculo, o veredito e "nao vale: usar so o probe". Numeros completos em `resultados/plano_bloco.md` (tabelas) e `resultados/plano_bloco.json`.

### Como rodar

`DADOS` = diretorio de dados (tmpfs, fora de `bench/`), `TRAB` = diretorio de trabalho. Tudo com `python3 -I`; rotulagem e avaliacao usam 6 processos nos P-cores fisicos 0,2,4,6,8,10 (nao dependem de tempo); medicoes de tempo (`custo-transform`, `tempo`) rodam sozinhas, 1 processo fixado na CPU 0:

```
python3 -I bench/preparar_dados.py --destino DADOS --zips ZIPS
python3 -I bench/plano_bloco.py autoteste
taskset -c 12 python3 -I bench/plano_bloco.py kernel --destino DADOS/kernel
taskset -c 0,2,4,6,8,10 python3 -I bench/plano_bloco.py rotular --dados DADOS --kernel DADOS/kernel --saida TRAB/rotulos.npz
taskset -c 0,2,4,6,8,10 python3 -I bench/plano_bloco.py features --dados DADOS --kernel DADOS/kernel --saida TRAB/features.npz
taskset -c 0 python3 -I bench/plano_bloco.py custo-transform --saida TRAB/transformes.json
taskset -c 0,2,4,6,8,10 python3 -I bench/plano_bloco.py avaliar --rotulos TRAB/rotulos.npz --features TRAB/features.npz --transformes TRAB/transformes.json --custo-transform numpy --saida TRAB/avaliacao_numpy.json
taskset -c 0,2,4,6,8,10 python3 -I bench/plano_bloco.py avaliar --rotulos TRAB/rotulos.npz --features TRAB/features.npz --transformes TRAB/transformes.json --custo-transform nativo --saida TRAB/avaliacao_nativo.json
taskset -c 0 python3 -I bench/plano_bloco.py tempo --dados DADOS --kernel DADOS/kernel --avaliacao TRAB/avaliacao_numpy.json --saida TRAB/tempo.json
python3 -I bench/plano_bloco.py relatorio --avaliacao TRAB/avaliacao_numpy.json --avaliacao-nativo TRAB/avaliacao_nativo.json --tempo TRAB/tempo.json --rotulos-meta TRAB/rotulos.npz.meta.json --transformes TRAB/transformes.json --kernel-manifesto DADOS/kernel_manifesto.json --json bench/resultados/plano_bloco.json --md bench/resultados/plano_bloco.md
```

`plano_bloco.py` importa `syntrabench.py` (funcao `fabricar`, mesmos codecs e opcoes da Rodada 1) e `economia.py` (`Contexto`, `agregar`, `perfil`, `liquido_gb`) por caminho de arquivo, sem copiar nem alterar. `autoteste` confere os transforms contra uma implementacao ingenua da semantica de `src/transform.rs` (846 casos, incluindo cauda). `avaliar` confere o valor liquido vetorizado contra `Contexto.liquido_gb` (diferenca maxima 0 US$/GB em 200 pares por granularidade).

### Dados (regenerados nesta rodada)

O diretorio de dados da Rodada 1 estava em tmpfs e havia sumido (a maquina reiniciou; `uptime` de 1h27 no inicio da rodada). Foi regenerado: Silesia por `preparar_dados.py`; Canterbury Large por `curl` (o `urllib` do script recebe HTTP 403 desse servidor; `curl` recebe 200) e extraido com as funcoes do proprio `preparar_dados.py`; os sinteticos pelo gerador do projeto. O sha256 dos 18 arquivos conferiu com o de `resultados/medicoes.json` (todos iguais, incluindo `ja_comprimido.xz`). Classe `codigo_kernel` (nova): o diretorio `bench-data-f1/` citado no pedido tambem nao existia; os primeiros 256 MiB do fluxo tar de `linux-6.12.115.tar.xz` (a mesma versao do Par A do gate F1) foram lidos em streaming de `cdn.kernel.org` com `lzma.LZMADecompressor` (34,6 MB de xz baixados), cabecalhos tar pulados (inclui pax e nomes longos GNU), so os bytes de conteudo foram guardados, agrupados por diretorio em 16 pseudo-arquivos (topo, ou segundo nivel para topos acima de 40 MiB; grupos menores que 8 MiB viram `__resto`); 242,6 MB de 32.427 arquivos regulares, incluindo `Documentation/` (texto), `arch/` e `drivers/`. Nunca executei nada do conteudo. O sha256 do `tar.xz` inteiro nao foi conferido porque so o inicio foi baixado (o lzma confere o CRC de cada bloco decodificado). Cada pseudo-arquivo do kernel e um "arquivo" para o leave-one-file-out.

Corpus final: 34 arquivos/grupos, 574,7 MB; blocos independentes de 64 KiB (8.782), 256 KiB (2.207) e 1 MiB (562), cauda final incluida (arquivos inteiros, nao fatias de 8 MiB). O corpus e dominado em bytes por `codigo_kernel` (42%), `sint_logs` (12%), mozilla/webster/nci; por isso o regret tambem e dado por classe e como media simples das classes.

### Candidatos, rotulos e bit-exatidao

Plano = (transform, codec). Caminho sincrono (9 planos): `stored`, `zstd-1`, `zstd-3`, e `d1`, `d2`, `d4`, `d8` (delta de largura 1, 2, 4, 8) e `b4`, `b8` (byte_split de largura 4 e 8) seguidos de zstd-3. Transforms implementados em numpy com a semantica de `src/transform.rs`: delta = diferenca por elemento little-endian com wrap modulo 2^(8w) (primeiro elemento menos zero), byte_split = n = len/w planos consecutivos de n bytes; em ambos os `len mod w` bytes finais passam sem transformacao. Caminho assincrono: `zstd-19`, `xz-9e` (dicionario LZMA2 do tamanho do bloco, CRC64, como na Rodada 1), `bzip2-9` e, so nos blocos em que o melhor transform vence zstd-3 por >= 3% (limiar `--limiar-transform`; escolha minha), `T+zstd-19` e `T+xz-9e` com o transform T vencedor do bloco.

Para cada (bloco, candidato) o programa comprime, decodifica, inverte o transform e compara o sha256 do bloco; tambem confere `invert(apply(x)) == x` isoladamente. Resultado: 139.652 comparacoes (106.188 + 26.678 + 6.786), 0 falhas de reconstrucao. Tamanho efetivo de um plano = `min(comprimido, bloco)` (se nao comprime, o motor grava `stored`; a tentativa continua custando CPU; mesma convencao de `economia.py`). O cabecalho do plano no envelope nao entra nos bytes.

Oraculos: (1) tamanho = menor tamanho efetivo entre os candidatos do caminho (empate: o plano mais barato). (2) economico = maior valor liquido por GB (`economia.py`: valor do armazenamento com T=60 meses menos custo de compressao menos n x custo de descompressao), para 2 cenarios (nuvem+S3 Standard, on-prem) x 3 camadas (n=100, 5, 0,1) = 6 oraculos. Vazoes por nucleo: `medicoes.json`, bloco de 1 MiB, por classe (media harmonica ponderada por bytes), a mesma para as 3 granularidades; `codigo_kernel` usa as vazoes de `codigo` (samba) (INFERENCIA PROPRIA). Plano com transform: vazao em serie, 1/(1/theta_zstd3_da_classe + 1/theta_transform), com theta_transform medido por mim em numpy (CPU 0, 1 MiB, mediana de 31): aplicar 19,4 a 19,5 GB/s (delta), 4,7 a 4,9 GB/s (byte_split); inverter 0,73 (d1), 1,4 (d2), 2,7 (d4), 5,1 (d8), 1,8 (b4), 3,4 (b8) GB/s. Esses custos sao de numpy, nao de Rust; uma sensibilidade com 4 GB/s (delta) e 3 GB/s (byte_split), valores INFERENCIA PROPRIA, esta em `plano_bloco.md` secao 15 (muda pouco o regret).

### Features (amostra do bloco, 21 numeros)

`h0` (entropia de ordem 0), `h1` (entropia condicional aproximada: contexto = 5 bits altos do byte anterior), `nd` (bytes distintos/256), `fz` (fracao de zeros), `fp` (fracao de ASCII imprimivel mais \t\n\r), `fmax` (maior frequencia de byte), `run` (log2 do comprimento medio de corrida), `hd1`, `hd2`, `hd4`, `hd8` (entropia de ordem 0 do fluxo de deltas de largura w), `hp4`, `hp8` (entropia media dos planos do byte_split; extra meu, util para a regra), `ac2`, `ac4`, `ac8` (taxa de igualdade x[i]==x[i-l]), `cr2`, `cr4`, `cr8` (autocorrelacao normalizada), `pr` (razao comprimido/original de zstd-1 sobre 4 KiB), `lenfrac` (tamanho do bloco / granularidade). Amostras testadas (triagem): primeiros 4 KiB, 4 pedacos de 1 KiB espalhados, primeiros 16 KiB, 4 pedacos de 4 KiB espalhados. Variante escolhida pelo menor regret medio nas 3 granularidades (CART, LOFO): **4 pedacos espalhados somando 4 KiB, com pesos por custo**. O `probe+zstd-3` usa a mesma amostra de 4 KiB (as duas politicas sao reportadas; o criterio usa a de menor regret).

### Modelos e validacao

CART (Gini, profundidade <= 6, minimo 10 amostras por folha, limiares nos 24 quantis do no, implementado em numpy), regressao logistica multinomial (z-score, L2 = 0,01, Adam 300 passos), regra manual (stored se h0 > 7,9; senao o transform com maior margem `h0 - hd_w` ou `h0 - hp_w` se a margem >= 0,5 bit; senao zstd-3). Rotulos de treino: plano mais barato dentro de 0,3% do melhor (reduz ruido de quase-empates); pesos de amostra = custo medio de errar (bytes, ou US$ x bytes) mais 2% da media; acuracia estrita avalia contra o argmin exato e a "tolerante" aceita plano a <= 0,5% do valor do oraculo. Treinados 1x por rotulo (tamanho + 6 economicos), 3 granularidades. Validacao: leave-one-file-out (todos os blocos de um arquivo, ou pseudo-arquivo do kernel, do mesmo lado) e leave-one-class-out. Bootstrap sobre os arquivos (2000 reamostragens) para o IC95 dos numeros do criterio. Tamanho: CART com 73/51/23 nos (64K/256K/1M, 16/16/8 features usadas); logistica com 22x9 = 198 pesos. Os parametros dos modelos finais (treinados em todo o corpus) estao em `plano_bloco.json`, portaveis para Rust (arrays de no ou matriz de pesos).

Linhas de base: `zstd-3 fixo`, `zstd-1 fixo`, `probe+zstd-3` (stored se a razao do zstd-1 sobre 4 KiB nao for <= 0,98, senao zstd-3), `oraculo`, `medir todos` (mesmos bytes do oraculo; custo = compressao de todos os outros candidatos, e tempo = soma das vazoes medidas).

### Medicao de custo

`tempo`: 1 processo fixado na CPU 0, GC desligado, por bloco de todo o corpus, mediana de 3 repeticoes de (a) zstd-3 do bloco, (b) as 21 features, (c) so as features que o CART final usa, (d) so o probe, (e) so as features da regra, (f) a inferencia (CART em Python puro sobre listas; logistica com numpy). As features sao numpy puro; a latencia inclui o overhead do interpretador e de ~20-40 chamadas numpy por bloco e e muito pior que a de uma implementacao nativa. Por isso o relatorio traz tambem uma estimativa nativa (INFERENCIA PROPRIA, nao medida): uma passada por feature distinta sobre a amostra a ~2 GB/s mais o probe zstd-1 medido.

### Resultados

Granularidade 64K / 256K / 1M. Regret de bytes vs oraculo do caminho sincrono (leave-one-file-out, global): zstd-3 fixo 1,52% / 1,58% / 1,12%; zstd-1 fixo 4,78% / 4,99% / 7,63%; probe+zstd-3 (espalhado) 1,59% / 1,72% / 1,41% (inicio: 1,68% / 2,21% / 1,91%); regra manual 3,03% / 4,11% / 3,35%; CART 0,62% / 1,66% / 0,47%; regressao logistica 0,39% / 0,76% / 0,80%. O teto de ganho de qualquer seletor sobre zstd-3 fixo e o proprio regret do zstd-3 fixo (1,1 a 1,6% dos bytes). O regret se concentra em `cientifico` (zstd-3 fixo perde 11 a 13%: vencem zstd-1, byte_split 4 e delta 2 conforme o bloco) e `dna` (3,4 a 3,6% em 64K e 256K, onde zstd-1 vence zstd-3 em todos os blocos); em texto, banco de dados, XML e kernel o zstd-3 fixo ja esta a < 0,7% do oraculo.

Criterio de parada (melhor classificador por granularidade; detalhe de todos os modelos em `plano_bloco.md`, secao 1):

| gran. | (i) regret de bytes <= 1% | (ii) ganho >= 1% sobre probe+zstd-3 | (iii) features+inferencia <= 2% do zstd-3 (numpy medido / nativo estimado) | veredito |
|---|---|---|---|---|
| 64K (logistica) | 0,39% (IC95 0,15 a 0,70) OK | 1,18% (IC95 0,07 a 3,42) OK | 123,6% / 37,0% FALHA | NAO VALE |
| 256K (logistica) | 0,76% (IC95 0,14 a 1,71) OK | 0,95% (IC95 0,12 a 2,59) FALHA | 31,2% / 8,8% FALHA | NAO VALE |
| 1M (CART) | 0,47% (IC95 0,11 a 1,05) OK | 0,92% (IC95 -0,02 a 2,80) FALHA | 3,2% / 1,4% FALHA no medido | NAO VALE |

O probe nao fica a <= 1% do oraculo em nenhuma granularidade (1,41% a 1,72%), entao a regra "usar so o probe" nao dispara; o veredito de "nao vale" vem das condicoes (ii) e (iii). Caso mais proximo de aprovar: 1M com CART (8 features, 23 nos): passa (i), fica a 0,08 ponto de (ii) e passaria (iii) com a estimativa nativa (1,4%), mas essa ultima e inferencia, nao medida. Em US$/GB (nuvem+S3, camada morna) o regret do zstd-3 fixo e 0,0069 / 0,0069 / 0,0047, o do probe (espalhado) 0,0073 / 0,0075 / 0,0059, o da logistica 0,0018 / 0,0033 / 0,0034 e o do CART 0,0028 / 0,0072 / 0,0020, contra 0,0002 de `medir todos` (que gasta 8x o tempo do zstd-3 so com os 9 candidatos sincronos e 173x com os 14). Treinar com o rotulo economico nao melhorou: regret em US$/GB (nuvem+S3, morno) de 0,0082 / 0,0079 / 0,0019 no CART e 0,0019 / 0,0034 / 0,0042 na logistica, contra 0,0028 / 0,0072 / 0,0020 e 0,0018 / 0,0033 / 0,0034 quando treinados em bytes. Transforms: so `cientifico` ganha de verdade (9,4 a 11,6% de bytes com o melhor transform escolhido POR BLOCO sobre zstd-3; um transform fixo por classe perde: byte_split 4 a -1,0% a -3,5%), `codigo` 0,4 a 1,2%, `binario` 0 a 0,8%, os demais 0% a 0,1%; o custo em vazao (numpy) e pequeno na compressao (348 -> 325 MB/s em cientifico com b4) e grande na descompressao (1.412 -> 786 MB/s), dominado pela inversao em numpy. Em leave-one-class-out o regret global sobe para 0,56 a 2,92% (logistica 0,56 / 1,05 / 1,72%; CART 0,82 / 2,92 / 2,10%).

### LIMITACOES desta rodada

- Corpus pequeno e de poucos arquivos: 18 arquivos reais/sinteticos mais 16 pseudo-arquivos de uma unica versao do kernel. `dna` (E.coli), `codigo` (samba), `banco_de_dados` (osdb), `xml` e `sint_logs` tem UM arquivo cada, entao o leave-one-file-out deles e na pratica leave-one-class-out; as classes com varios arquivos sao texto (5), cientifico (4), binario (2) e o kernel (16 grupos que compartilham estilo de codigo, o que torna o LOFO otimista nessa classe). `sint_*` sao sinteticos e entram no regret global. O regret depende de poucos arquivos: os IC95 por bootstrap de arquivos tem 1 a 3 pontos de largura e a escolha da variante de amostragem mexe o regret do CART em 1 a 2 pontos (triagem na secao 4 de `plano_bloco.md`).
- As vazoes por classe vem do benchmark anterior (blocos de 1 MiB, fatias de 8 MiB, 4 trabalhadores concorrentes, notebook hibrido) e valem para as 3 granularidades; a vazao do kernel e a de samba; nao ha vazao medida por bloco.
- Custo de transform: numpy medido (rotulado) e estimativa nativa (INFERENCIA PROPRIA). Custo de features: numpy medido (pessimista) e estimativa nativa (INFERENCIA PROPRIA); nenhuma implementacao em Rust ou C foi medida.
- zstd via `compression.zstd` do Python 3.14 (zstd 1.5.7, sem checksum, quadro com cabecalho de ~9 a 13 B por bloco), xz com dicionario do tamanho do bloco e CRC64, bzip2 1.0.8; tamanhos incluem esses cabecalhos; o cabecalho de plano do envelope nao entra.
- Limiar de 3% para tentar T+xz/T+zstd-19, tolerancia de 0,3% nos rotulos de treino, pesos por custo, margem 0,5 bit da regra, L2, profundidade 6 e a politica de escolha da variante sao escolhas minhas; o classificador assincrono (escolha entre zstd-19, xz-9e, bzip2-9, T+...) NAO foi treinado (so o oraculo assincrono foi calculado, secao 3.2 de `plano_bloco.md`: 14 a 19% menor que o oraculo sincrono).
- O corte "nao vale" depende de numpy: com features nativas o custo cai uma a duas ordens de grandeza, e o 1M poderia passar (iii); isso exige medir uma implementacao nativa antes de reabrir a decisao. A condicao (ii) esta no limite (IC95 cruza 1%) em 64K e 1M.
- O sha256 do `tar.xz` do kernel nao foi conferido (so o inicio foi lido); o reinicio da maquina apagou os dados da Rodada 1, regenerados com sha256 identico ao registrado.
- Tempo: as fases de calculo somam ~5 min (rotulagem 134 s, features 2 s, avaliacao 59 s x 2, tempo 31 s); o orcamento de 60 min de relogio foi excedido (regeneracao dos dados, desenvolvimento e depuracao).

## Rodada 4: capacidade local sustentada, `capacidade_local.py` e `membw.c`, execucao de 2026-11-04

Pergunta: quanto de 5 PB/mes (1.929 MB/s medios; pico x4 = 7.716 MB/s, ou 20 PB/mes) esta maquina sustenta de verdade? As escalas das rodadas 1 e 2 sao rajadas de 1,5 s; um notebook reduz clock e potencia sob carga prolongada e o disco tem um regime de rajada (cache SLC) diferente do regime sustentado. A rodada mede 10 minutos por configuracao, calibra 1 nucleo contra uma referencia publica, mede banda de memoria e mede I/O real de NVMe com O_DIRECT. Os numeros alimentam `docs/capacidade-local.md`.

### Como rodar

`DADOS` e o diretorio de dados da Rodada 1 em tmpfs (manifesto `manifesto.json`; o sha256 dos 18 arquivos e conferido contra `medicoes.json` em cada fase), `TRAB` e um diretorio de trabalho em tmpfs. Tudo com `python3 -I`. Cada fase grava uma chave de `resultados/capacidade_local.json`; `resumo` gera `resultados/capacidade_local.md`. Um teste por vez (nenhuma outra carga nossa em paralelo):

```
python3 -I bench/capacidade_local.py inventario --dados DADOS --trabalho TRAB
python3 -I bench/capacidade_local.py calibracao --dados DADOS --trabalho TRAB
python3 -I bench/capacidade_local.py memoria --dados DADOS --trabalho TRAB
python3 -I bench/capacidade_local.py sustentado --dados DADOS --trabalho TRAB --descanso-inicial
python3 -I bench/capacidade_local.py io-disco --dados DADOS --trabalho TRAB
python3 -I bench/capacidade_local.py resumo --dados DADOS --trabalho TRAB
python3 -I bench/capacidade_local.py tudo --dados DADOS --trabalho TRAB --descanso-inicial
```

`tudo` roda inventario, calibracao, memoria, sustentado (com descanso de 120 s antes da primeira configuracao e de 180 s entre as demais), 60 s de descanso, io-disco e resumo. Opcoes: `--duracao` (600 s), `--intervalo` (5 s), `--descanso` (180), `--configs` (`A_comp,B_comp,C_comp,B_decomp`), `--repeticoes` (3, calibracao), `--mem-repeticoes` (5), `--teto-gib` (6), `--io-dir` (`~/.cache/syntra-capacity`, apagado ao final), `--io-gib` (16), `--io-cap-mib` (512), `--io-plano` (`modo:N:variante:segundos,...`, padrao `comp:8:odirect:180,comp:16:odirect:180,comp:16:bufferizado:60,ram:8:cpu:25,ram:16:cpu:25,decomp:16:descartar:90,decomp:16:grava:90,decomp:8:descartar:45`), `--io-teto-leitura` (10 s), `--io-teto-escrita` (60 s), `--io-descanso` (30 s).

### Definicoes

- PB/mes equivalente = MB/s x 2,592e6 s / 1e9 (mes de 30 dias). Alvo medio 5 PB/mes = 1.929 MB/s; pico x4 = 7.716 MB/s = 20 PB/mes sustentado. "% de 5 PB/mes" = MB/s / 1.929; "% do pico x4" = MB/s / 7.716. MB/s e sempre de bytes LOGICOS (entrada da compressao; saida da descompressao), 1 MB = 1e6 B.
- Configuracoes sustentadas: A = 8 P-cores fisicos (CPUs 0,2,...,14); B = 8 P fisicos + 8 E-cores (N = 16: 0,2,...,14,16-23); C = 32 threads (todas as CPUs). B tambem em descompressao. Um processo por CPU, fixado com `sched_setaffinity`; o processo pai (amostrador) fica fixado na CPU 31 (E-core; em C ele divide a CPU com um trabalhador, ~0,4% de uma CPU).
- Rajada = media dos primeiros 30 s (da curva de bytes acumulados, interpolada em 30 s); sustentado = media dos ultimos 300 s; derating = sustentado / rajada; minimo e maximo sao de janelas de 5 s. Em execucoes curtas de teste as janelas viram 1/10 e 1/2 da duracao (so para fumaca).

### Carga e dados do teste sustentado

zstd-3 em blocos independentes de 1 MiB, mesma fabricacao de `syntrabench.py` (`fabricar("zstd-3", 1 MiB)`, `pool_blocos`, `ler_fatia`): pool = todos os blocos de 1 MiB alinhados dos 15 arquivos reais (sem sinteticos), 204 blocos; o processo i de N recebe `pool[i::N]` (dados distintos por processo; A: 25 a 26 blocos por processo, B: 12 a 13, C: 6 a 7). O setup de cada processo comprime todos os blocos, descomprime e confere sha256 (bit-exatidao), e so depois entra na barreira; a descompressao usa os mesmos blocos ja comprimidos em memoria. Cada processo atualiza um contador de bytes (8 B) em memoria compartilhada (`mmap` anonimo antes do `fork`) a cada bloco; o pai le os contadores a cada 5 s. O prazo e lido de memoria compartilhada a cada 8 blocos (permite abortar). ATENCAO: a razao zstd-3 deste pool e 3,18 (inclui arquivos inteiros muito compressiveis, como `nci`), nao 2,57 do `mix_real` de `medicoes.json` (fatias de 8 MiB); em compressao, dado mais compressivel roda mais rapido por byte de entrada. O fator de conteudo foi medido com um controle sem I/O no mix de 2,56 (ver abaixo) e esta no resumo.

### Amostragem a cada 5 s (`Amostrador`)

Por amostra: vazao agregada; `scaling_cur_freq` das 32 CPUs (media por tipo, nos CPUs ativos do teste e em todos); todas as `thermal_zone*/temp` com o tipo, mais o maximo de `coretemp` e a temperatura do NVMe (hwmon); contadores `thermal_throttle/core_throttle_count` (soma dos 32 CPUs), `core_throttle_total_time_ms`, `package_throttle_count` e `package_throttle_total_time_ms` (CPU 0) cumulativos (o resumo reporta incrementos); energia RAPL se legivel; `/proc/diskstats` do `nvme0n1` (MB/s de leitura e escrita, `io_ticks` como utilizacao, fila media); `MemAvailable` e swap usado; `loadavg`. Abortar um teste: temperatura do pacote >= 103 C, swap crescendo mais de 1 GiB ou menos de 1.000 MiB disponiveis (nenhuma dessas condicoes ocorreu se o campo `interrompido` for nulo).

Carga de fundo (o que NAO e nosso): varredura de `/proc/<pid>/stat` (utime+stime) a cada amostra; os processos nossos sao os descendentes do pai (identificados por `ppid`), o resto e fundo. `bg_cores_proc` = soma do CPU dos processos nao nossos (em nucleos-equivalentes, todos os CPUs); `bg_no_conjunto_cores` = CPU ocupada nos CPUs do teste e nos irmaos SMT dos P-cores usados (de `/proc/stat`) menos o CPU dos nossos trabalhadores (o que mais atrapalha a medida); `bg_top` = os 3 maiores por nome de processo (por exemplo `chrome`). Tambem medida por 3 s antes de iniciar os trabalhadores e por 3 s depois de terminarem. A correlacao entre `bg_no_conjunto_cores` e a vazao por janela de 5 s esta em `ruido_de_fundo`. Descansos ociosos entre testes sao amostrados a cada 15 s (temperatura, frequencia, fundo e incremento dos contadores de throttle em repouso).

### Calibracao de 1 nucleo

`silesia.tar` = tar GNU nao comprimido dos 12 arquivos do Silesia em ordem de nome (`tarfile`, mtime 0, modo 0644), em tmpfs; `taskset -c CPU zstd -T1 -b1 -e3 -i3` (CLI 1.5.7 do sistema), 3 repeticoes intercaladas em CPU 0 (P) e CPU 16 (E), mediana. Referencia publica informada no pedido (README do zstd 1.5.7, i7-9700K, lzbench, Silesia): nivel 1 razao 2,896, compressao 510 MB/s, descompressao 1.550 MB/s. Fator = nossa vazao / referencia. LIMITACAO: base nao identica (o `silesia.tar` da referencia e o do site, este foi montado aqui com `tarfile`; cabecalhos e padding podem diferir em poucos KB); CLI com `-T1` e `taskset`; a referencia e um i7-9700K a 4,9 GHz em Ubuntu 24.04. A referencia so existe para o nivel 1; o nivel 3 e reportado sem fator.

### Memoria (`membw.c`)

Cada thread aloca 3 vetores de `double` (mmap anonimo, primeiro toque na propria thread fixada), threads sincronizadas por barreira a cada repeticao; copy `c[i]=a[i]` (16 B/elemento) e triad `a[i]=b[i]+3*c[i]` (24 B/elemento), contagem estilo STREAM (sem contar write-allocate); uma repeticao de aquecimento descartada, 5 repeticoes medidas, melhor e mediana; tempo = do menor inicio ao maior fim entre as threads. `-fno-tree-loop-distribute-patterns` impede o gcc de transformar o copy em `memcpy` (que usa stores nao temporais acima de um limiar e daria numero diferente). Configuracoes: 1, 2, 4, 8 P fisicos, 8 P + 8 E, 32 threads. DESVIO do pedido: vetores de 512 MiB por thread so valem ate 4 threads (3 x 512 MiB x 32 = 48 GiB nao cabe nos 30 GB com a estacao em uso); o tamanho cai para 256, 128 e 64 MiB com 8, 16 e 32 threads, mantendo ~6 GiB de conjunto de trabalho (cada vetor ainda e >= 64 MiB contra 36 MiB de L3). A comparacao com o zstd usa trafego = vazao sustentada x (1 + 1/razao) (entrada lida uma vez + saida escrita uma vez; INFERENCIA PROPRIA, ignora o estado interno do zstd e o write-allocate).

### I/O de disco (`io-disco`)

Arquivos em `~/.cache/syntra-capacity/` (NVMe; apagados ao final, inclusive em caso de erro). Entrada de 16 GiB gerada com semente 20261104: cada bloco de 1 MiB vem de um offset aleatorio (nao alinhado) dentro dos 8 MiB iniciais de um arquivo real sorteado com peso proporcional ao tamanho da fatia de 8 MiB (mesma definicao de `mix_real`; sorteio com pesos pelo arquivo inteiro daria razao 3,28, sorteio com offsets em todo o arquivo 2,77, dentro dos 8 MiB iniciais 2,56 = alvo ~2,5), com 16 bytes aleatorios mutados por bloco. A razao zstd-3 e registrada em amostra de 1/16 dos blocos na geracao e exata (todos os blocos) no preparo do comprimido.

Pipeline de compressao: cada processo le a sua regiao (1/N do arquivo, circular) com `O_DIRECT` em blocos de 1 MiB alinhados (`preadv`, fila 1 por processo, sincrono), comprime com zstd-3, empacota quadros `[u32 tamanho original][u32 tamanho comprimido][dados]` em um buffer de 1 MiB e o grava no arquivo de saida do proprio processo (`pwrite` de 1 MiB; arquivo circular de 512 MiB por processo: a primeira volta estende o arquivo, as seguintes sobrescrevem). Variantes de saida: `odirect` (O_DIRECT) e `bufferizado` (sem O_DIRECT, sem `fsync` periodico, `posix_fadvise(DONTNEED)` a cada 128 MiB e `fdatasync` no fim; o tempo do flush final e reportado). Caminho de descompressao: cada processo le sequencialmente (O_DIRECT, 1 MiB) os arquivos comprimidos que lhe cabem (16 arquivos produzidos no preparo; N=16: um arquivo por processo, N=8: dois), desempacota e descomprime; variantes `descartar` e `grava` (grava o bloco descomprimido de volta no NVMe com O_DIRECT). N = 8 P fisicos e N = 16 (8P+8E). Controle `ram`: compressao em memoria de 48 blocos do mesmo arquivo, sem I/O, para separar custo de I/O de efeito de conteudo. Tetos do disco com os mesmos processos: leitura pura (10 s) e escrita pura (60 s, N=16, O_DIRECT, 1 MiB, fila 1 por processo); a segunda metade do teto de escrita e o "teto sustentado".

Por teste: vazao de entrada (depois de 10 s de aquecimento, tambem da segunda metade e incluindo o flush final), MB/s de leitura e escrita medidos em `/proc/diskstats`, utilizacao e fila media do disco, `cpu_frac` (CPU de usuario+sistema do processo / tempo de relogio, media dos processos; ~1,0 = limitado por CPU, bem abaixo = esperando I/O), RSS (`VmHWM` e `VmRSS` de cada processo), latencias p50/p99/p99,9 de: bloco de 1 MiB (iteracao inteira: leitura + CPU + escrita quando disparada por aquele bloco), CPU do bloco, cada leitura de 1 MiB e cada escrita de 1 MiB. LIMITANTE (heuristica, INFERENCIA PROPRIA): CPU se `cpu_frac` >= 0,90, disco abaixo de 80% do teto e utilizacao < 85%; disco se `cpu_frac` < 0,85 e (disco >= 80% do teto ou utilizacao >= 85%); senao "misto/indefinido".

Interpretacao do limite "<= 40 GiB escritos no total": lido como ESPACO OCUPADO simultaneamente em disco (16 GiB de entrada + ~6,4 GiB de comprimido + 16 x 512 MiB de saidas = ~30,5 GiB), porque a letra (40 GiB de bytes escritos) e incompativel com testes de 3 minutos, que escrevem centenas de GB. O volume fisico escrito e lido na fase (de `/proc/diskstats`) e registrado em `io_disco.escrita_fisica_total_GiB`.

### Resultados

Ver `resultados/capacidade_local.md` (tabela unica e detalhe por teste) e `resultados/capacidade_local.json` (curvas de 5 em 5 s).

### LIMITACOES da Rodada 4

- ESTACAO EM USO (Chrome, VS Code, gnome-shell, Claude Code): a carga de fundo e medida e reportada por amostra, mas nao e removida. Em A e B ela cai sobretudo nos E-cores livres e em CPUs fora do conjunto; em C (32 threads) o fundo disputa os mesmos CPUs dos trabalhadores. Os numeros sao de UMA execucao de 10 min por configuracao, sem repeticao entre dias.
- Notebook hibrido com potencia e termica limitadas, governor `powersave` (EPP `balance_performance`), em AC, perfil de energia `balanced`; limite de potencia do pacote 200 W (PL1 = PL2, legivel); a temperatura do pacote chega perto do limite termico nos testes com mais nucleos. Nao ha como separar throttle por temperatura de queda de clock por corrente/potencia com os contadores legiveis.
- Dados: o pool do teste sustentado (razao zstd-3 3,18) e as fatias de 8 MiB do mix de I/O (razao ~2,56) sao amostras de 15 arquivos publicos (Silesia e Canterbury Large); nao representam o dado real do produto. O arquivo de 16 GiB de entrada e sintetico no sentido de ser reamostrado de ~99 MiB de conteudo real (blocos de 1 MiB de offsets aleatorios nos 8 MiB iniciais de cada arquivo, com 16 B mutados por bloco), entao ha repeticao de conteudo entre blocos (irrelevante para blocos independentes, mas relevante para qualquer dedup).
- O_DIRECT em fila 1 por processo (leitura e escrita sincronas, sem leitura antecipada nem sobreposicao de I/O e CPU dentro do processo): a profundidade de fila do disco e ~N, nao a que um motor com I/O assincrono usaria; os resultados de I/O podem ser pessimistas para disco e para o encaixe de CPU. Cada processo le uma regiao propria do arquivo e grava um arquivo proprio, mas o sistema de arquivos (ext4, journal), o controlador do NVMe e as interrupcoes sao compartilhados.
- O NVMe tem regime de rajada (cache SLC) e regime sustentado; o estado do disco antes de cada teste (quanta escrita recente, 68% de ocupacao) afeta o resultado. A ordem dos testes e os descansos (30 s) entre eles nao recuperam a cache SLC por completo; a curva de 5 s de cada teste mostra a transicao quando ela ocorre. O teste de escrita de 60 s tambem pode nao chegar ao regime estacionario de horas.
- Nao ha rede, nem ingestao, nem metadados, nem erasure coding, nem fsync por objeto: so bloco comprimido a bloco no disco local. O preparo do comprimido e o arquivo de entrada nao fazem parte das medidas de vazao.
- RAPL: `energy_uj` ilegivel (permissao negada) nesta execucao; so os limites de potencia (`constraint_*_power_limit_uw`) sao legiveis. ENERGIA NAO MEDIDA; nao existe J/GB nem W medido. A bateria esta cheia em AC, e nao e proxy de consumo.
- A fabricacao de blocos usa `compression.zstd` do Python 3.14 (zstd 1.5.7, uma chamada one-shot por bloco, sem checksum), como nas rodadas anteriores; o CLI mede ate ~5 a 13% a menos em compressao (secao de conferencia cruzada da Rodada 1).
- Memoria: vetores menores que 512 MiB acima de 4 threads (ver acima); contagem estilo STREAM sem write-allocate; o fundo da estacao tambem usa banda.
- Calibracao: base nao identica a da referencia publica (ver acima); um unico processo CLI, 3 repeticoes.
- Orcamento: o pedido era <= 90 min de relogio, mas os testes sustentados somam sozinhos ~50 min; ver o resumo ao final do relatorio para o tempo realmente gasto.
