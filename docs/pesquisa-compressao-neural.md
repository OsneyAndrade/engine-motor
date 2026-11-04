# Memória do dado, compressão neural e armazenamento: levantamento técnico e viabilidade para o Syntra Engine

Data: 2026-11-03 · Repositório: `engine-motor` (Syntra Engine v0.3.0) · Requisito acrescentado: **5 PB/mês**, aplicável ao I/O de data centers, com viabilidade matemática.

## Resumo executivo

1. **O projeto atual não contém rede neural.** É um motor clássico (zstd/xz/brotli/bzip2/LZ4 + transforms + dedup BLAKE3 de arquivo inteiro) com garantia bit-exata sustentada por decoders determinísticos. A tese "armazenamos uma memória e uma rede neural reconstrói" é uma **hipótese de pesquisa**, não um componente existente. O "~60%" é plausível só para dados estruturados; o corpus de 85 MB que o sustenta é próprio e não público (Seção 1).
2. **A teoria permite, mas limita.** Um modelo melhor só se aproxima da entropia real; nenhum modelo comprime dado de entropia máxima; mover bits para os pesos não é compressão (o decodificador conta). Para "memória + reconstrução" bit-exata, o custo honesto é `|dado codificado| + |modelo|` (Seção 2).
3. **O ganho neural genérico é pequeno e lento.** Em texto, com o modelo contado e fora de distribuição, a vantagem sobre `zstd -22`/LZMA2 é de **~3–8 pontos percentuais**; para binários, bancos e logs não há evidência; para imagem lossless o ganho sobre JPEG-XL é ≤0,25 bpsp. A vazão medida vai de **0,0001 a 4 MB/s por GPU**, contra 510–1.550 MB/s por núcleo do `zstd -1` (Seções 3 e 7).
4. **Reconstrução bit-exata em rede neural em ponto flutuante falha entre hardwares:** nenhum dos 150 arquivos do teste PMATIC decodificou entre dois Macs; 47% de pixels bit-exatos entre H100 e L4 (LatentStore); NVIDIA não garante bit a bit entre arquiteturas. Corrigir custa +0,5–7,9% de taxa, ou 4–5x de velocidade, ou 3,2x no tamanho (Seção 5).
5. **Break-even:** com preços de 2026, a troca do clássico pelo neural tem margem **negativa em 4 de 6 cenários**; os 2 positivos exigem ≥1,9 PB em S3 Standard (≥92 PB on-prem) e leitura quase nula. Processar gasta **14–71x** mais energia do que o armazenamento economizado (Seções 4 e 6).
6. **5 PB/mês = 1,93 GB/s médios (7,7 GB/s no pico ×4).** `zstd -1` e `zstd -3` cumprem com 26 a 48 núcleos no pico (3,3 a 5,3 em média, ~US$107–171/mês), **medidos**; neural exige **482 a 7.716 GPUs** só para a média. O gargalo escondido é o **índice de dedup** (19,5 TB por mês de dados a 8 KiB/bloco) (Seção 9).
7. **Benchmark medido (Seção 10, i9-14900HX, dados públicos Silesia/Canterbury, 0 violações de bit-exatidão):** só `zstd -1` e `zstd -3` cumprem 5 PB/mês com ≤64 núcleos no pico (26 e 48 núcleos); a eficiência de escala cai para 0,42 a 32 threads; blocos de 1 MiB custam 0,3–1,3% de razão e 64 KiB 4,5–10%; a recompressão de zstd-3 para zstd-19 se paga em 3,7 meses (S3) a 21,5 meses (Glacier IR). A preço de nuvem a CPU custa 0,1–6% do valor guardado: o limite é vazão, não custo. **Rodada 2 (Seção 10.8):** o gate F1 do dedup por bloco (CDC) **reprova em árvores de arquivos** (ganho extra de −15% a 0% sobre dedup de arquivo inteiro + zstd-3) e só passa em fluxos únicos, onde `zstd --long` e o delta entre versões o superam; em núcleos homogêneos a eficiência de escala é 0,72–0,91 até 16 núcleos.
8. **Onde a evidência é forte:** dedup por bloco + compressão clássica; recompressão específica de formato (Lepton: −22,7% em 203 PiB reais); conteúdo gerado pelo mesmo modelo que o reconstrói (LatentStore: −78,7% vs PNG). **Onde a rede neural caberia sem tocar o bit-exato:** no plano de controle (escolher transform/codec/nível por bloco). **Medido em 2026-11-04 (T1.2, Seção 10.9): o teto de ganho é de 1,1–1,6% dos bytes e o critério de parada não foi atendido; não vale implementar agora.** O valor está concentrado em dados numéricos/científicos, onde `zstd-3` fixo perde 11–13% para o melhor transform por bloco.
9. **Recomendação (a retirada da rede neural do caminho de dados foi confirmada pelo Osney em 2026-11-04):** aposentar "NN substitui o codec" como tese genérica; construir o diferencial em torno de **planejamento econômico medido + dedup de arquivo inteiro com referência/delta entre versões + recompressão em camadas + integridade provada**, com uma camada fria neural opcional atrás de gates eliminatórios (Seções 8 e 10).
10. **Mercado (varredura de 2026-11-04, Seção 10.5):** não achei produto com as quatro peças juntas, mas três existem separadas: **Meta OpenZL** (plano treinado por formato, decoder universal, BSD), **SCOPe e a patente US11990923** (escolha de codec/tier por custo em US$) e **cobrança por economia** em FinOps e, em lakehouse, na Granica. A hipótese de diferencial passa de forte para parcial; **OpenZL vira baseline obrigatório e possível componente**, e a patente exige análise jurídica antes de comercializar.

### Nota de método e limites

- O benchmark da Seção 10 foi construído e executado por um quinto subagente; conferi por amostragem (0 violações, 0 falhas, ausência de comentários no código, validação à mão, ordem de grandeza da vazão) e **não reproduzi a execução inteira**.
- A pesquisa bibliográfica foi feita por **quatro subagentes** (compressão visual/implícita; compressores LLM e dedup semântica; storage/custos/energia; falhas e determinismo); eu li o código do projeto, conferi DOIs clássicos no Crossref, confirmei a existência de PMATIC, Nacrith e Lepton em busca independente, refiz as contas derivadas em scripts e corrigi dois erros (unidade do xz na tabela de núcleos; ID arXiv de Atil et al.). **Não reabri cada PDF**: os números rotulados [V] foram lidos pelos subagentes.
- Referências de 2026 estão fora do meu conhecimento prévio; **confira antes de citar externamente** (Bibliografia, no início).
- Contas "(deriv.)" e premissas "(inf.)" são minhas e **não revisadas por terceiros**. Parâmetros sem fonte (custo de pesquisa, durações de treino, preço on-prem tudo-incluído) estão marcados como tal; mudam a escala D\*, **não o sinal** da margem.
- Preços e custos são de **2026-11-03** e voláteis (NAND/DRAM subiram ~3,6x em 2026).
- A varredura de concorrentes (Seção 10.5) foi feita em 2026-11-04 por um subagente; ficaram sem verificação o preço atual da Granica, os termos primários de NetApp, Hitachi e IBM, Veeam, Rubrik, HPE, Qumulo, Cloudflare e Snowflake. A análise de patente **não é parecer jurídico**.

---

## 1. Levantamento do projeto atual

Fonte: `README.md`, `Cargo.toml`, `proto/engine.proto` e o código em `src/` (v0.3.0, ~11,5 mil linhas de Rust), lidos em 2026-11-03; mais a visão de produto registrada pelo Osney (2026-09-15).

### 1.1 O que existe

| Camada | Estado real no repositório |
|---|---|
| Compressão | Motor **clássico** de seleção: classifica o conteúdo, gera 1/4/16 planos (transform × codec × nível × dicionário), **mede** todos sobre o próprio dado e fica com o menor. Codecs: stored, LZ4, DEFLATE, zstd, bzip2, brotli, xz. Transforms: `delta`, `byte_split`, `rle`, `csv_columnar`. |
| Integridade | Envelope `.syntra` (protobuf v3) com BLAKE3 de 256 bits do original, conferido em toda descompressão; checksum do payload (64 bits truncados de BLAKE3); *verify-on-write* (descomprime e compara antes de gravar); piso `stored` (payload nunca maior que o original). |
| Dedup | Global, **por arquivo inteiro**, endereçado por conteúdo (BLAKE3). Dedup em nível de bloco (CDC) está modelado no proto (`Block`) mas **não ativado**. |
| Dicionários | zstd treinado por classe de conteúdo, endereçado por digest, armazenado **fora** do envelope. |
| Produto | Multi-tenant, chaves/escopos, ledger append-only de consumo, duas fórmulas de cobrança (`savings_share`, `ingested_gb`), ingestão por diretório, Prometheus, SQLite/Postgres, sled, Redis. |
| **Rede neural / modelo aprendido** | **Não existe.** Busca por torch/onnx/candle/embedding/neural/token/vector no código e no `Cargo.toml`: zero ocorrências. O README declara codecs perceptuais, quantização e compressão de modelos/vetores como *fora de escopo*. |
| Eixo "tokens de IA" (LM cache, embeddings, índice vetorial) | **Não existe** em código; só aparece na visão de produto. |

### 1.2 Decisões de arquitetura relevantes para este estudo

1. **Medir em vez de assumir**: o plano vencedor sai de compressão real do dado, não de heurística fixa.
2. **Bit-exato como contrato**: lossy é recusado por desenho (auditoria, vídeo de crime).
3. **Segurança por redundância**: duas verificações independentes (na gravação e na leitura).
4. **Receita de reconstrução dentro do envelope**, mas **dicionário fora** → o envelope não é 100% autossuficiente (já existe um "ponto único de falha externo": `409 dictionary_unavailable`). Esse é o análogo em pequena escala do risco "perder o modelo" da Seção 5.
5. Tudo carregado **em memória**: corpo HTTP como `Bytes` (limite 4 GiB, `SYNTRA_MAX_BODY_MB=4096`) e `std::fs::read` na ingestão por diretório. Não há streaming.

### 1.3 Métricas e resultados preliminares (do README)

Corpus de 85,0 MB, 6 arquivos (CSV, NDJSON, SQL dump, `i64`, `f32`, bytes aleatórios):

| Modo | Economia | Tempo (85 MB) | Throughput derivado* |
|---|---:|---:|---:|
| política anterior (LZ4/zstd-5) | 53,3% | — | — |
| `fast` | 70,9% | 0,5 s | ≈ 170 MB/s |
| `balanced` | 76,1% | 11,3 s | ≈ 7,5 MB/s |
| `max` | 78,8% | 130,4 s | ≈ 0,65 MB/s |

\* *Inferência própria:* 85 MB ÷ tempo. O README não informa CPU, núcleos nem RAM.

Extrapolação linear para os **5 TB** do requisito (inferência própria, hardware não declarado): `fast` ≈ 8 h; `balanced` ≈ 7,7 dias; `max` ≈ 89 dias de relógio. É o *piso de comparação* de throughput para qualquer abordagem neural.

### 1.4 Premissas

| # | Premissa (explícita ou implícita) | Status | Justificativa |
|---|---|---|---|
| P1 | Reconstrução bit a bit é garantida | **Verificada** | BLAKE3 conferido em toda leitura + verify-on-write + testes de roundtrip. Vale porque os decoders clássicos são determinísticos por especificação; é exatamente essa propriedade que não se herda de graça numa rede neural em ponto flutuante (Seção 5). |
| P2 | O sistema armazena uma "memória" e a reconstrói com uma rede neural nossa | **Falsa no estado atual** | Não há NN no código. Hoje é compressão clássica + dedup. A NN é uma *hipótese de pesquisa*, não um componente existente. |
| P3 | ~60% de redução é um resultado geral ("1 GB vira ~60% menor") | **Provavelmente falsa como regra geral** | O 78,8% do README vem de um corpus favorável (5 de 6 arquivos estruturados); o arquivo aleatório deu 0%. Mídia já comprimida, criptografados e executáveis ficam longe disso (Seção 7). Em dados estruturados/redundantes, é plausível — com compressão **clássica**. |
| P4 | O corpus de 85 MB representa a carga real | **Não verificada** | Não está no repositório (busca no disco não achou `vendas.csv`), não é benchmark público, 6 arquivos, sem variância. |
| P5 | Os tempos do README valem para a máquina-alvo | **Não verificada** | Hardware não declarado. |
| P6 | Decidir o plano por uma amostra de 8 MB vale para o arquivo inteiro (>48 MB) | **Não verificada, provavelmente falsa para dado heterogêneo** | `probe_slice` pega **uma** janela contígua central. Imagem de VM, `tar`, binário com seções e logs com mudança de regime violam a hipótese. |
| P7 | O motor escala a 5 TB on-premise | **Não verificada** | Sem streaming nem CDC; objeto único ≤ 4 GiB e inteiro em RAM; `max` a ~0,65 MB/s. |
| P8 | Dedup por arquivo inteiro captura a redundância relevante | **Provavelmente falsa** | Backups, imagens de VM, bancos e repositórios têm redundância *entre versões parciais*; o ganho de dedup por bloco costuma ser maior (Seção 4). |
| P9 | "Essência" menor reduz custo e I/O | **Parcialmente verificada** | Reduz bytes armazenados; mas **leitura exige descompressão** (CPU/GPU) — o ganho de I/O só existe se o gargalo for banda de armazenamento, não CPU (Seção 6). |
| P10 | Redução de tokens de IA decorre do mesmo mecanismo de compressão | **Não verificada e provavelmente falsa** | Tokens de LLM são contados sobre o texto/ids que o modelo *consome*; um blob comprimido por zstd/NN não reduz tokens — precisa ser descomprimido antes. Redução de tokens é outro problema (seleção/sumarização de contexto, cache de prefixo/KV). Fora do escopo desta pesquisa. |

### 1.5 Lacunas

- Nenhum experimento com **modelo aprendido** como compressor (nem baseline simples, como um LM pequeno + codificador aritmético).
- Nenhum benchmark público (Silesia, Canterbury/Large, enwik8/enwik9, Calgary, Kodak/CLIC, corpora de logs/CSV/Parquet reais).
- Nenhuma medição de **energia** (Wh/GB) nem de **custo por GB** (CPU-hora), o que impede fechar break-even.
- Nenhum teste de **corrupção injetada** em essências (bit flip em payload tem teste; truncagem, setor ruim e perda de dicionário não).
- Sem requisito numérico de **latência de leitura** por classe de dado (quente/frio).
- Sem política de **versionamento de modelo/dicionário** no envelope além do digest do dicionário.

---

## 2. Fundamentos teóricos (com fontes)

> Referências desta seção são clássicas; os metadados bibliográficos de Shannon (1948, 1951), Witten–Neal–Cleary (1987), Rissanen (1978) e Li–Vitányi (2019) foram **conferidos no Crossref em 2026-11-03**. As demais foram citadas de memória (autor/ano/veículo) e estão marcadas.

### 2.1 Entropia de Shannon e o teto da compressão sem perdas

Para uma fonte com distribuição *p*, a entropia é H(X) = −Σ p(x) log₂ p(x). O **teorema da codificação de fonte** (Shannon, 1948) diz que nenhum código sem perdas tem comprimento médio abaixo de H bits/símbolo, e que códigos de prefixo alcançam H ≤ L < H+1; com blocos longos (ou codificação aritmética) chega-se arbitrariamente perto de H. Para fontes com memória vale a **taxa de entropia**.

Consequências diretas para o projeto:

- O limite não é do algoritmo, é **da fonte**: nenhum codec — clássico ou neural — passa abaixo da entropia verdadeira. Um modelo melhor só **aproxima** a entropia real melhor do que um modelo pior. Ganho máximo possível de qualquer compressor sobre outro = diferença entre os dois cross-entropies sobre aquele dado.
- Shannon (1951) estimou a entropia do inglês escrito entre ~0,6 e ~1,3 bit/caractere: comparado a 8 bit/char, isso sugere um teto de redução da ordem de 85–92% **para quem tiver um modelo de língua extraordinariamente bom** (inferência própria: o alfabeto de Shannon tinha 27 símbolos, sem caixa nem pontuação, então a cifra é só ordem de grandeza). Compressores práticos ficam muito acima desse piso; essa folga é onde modelos neurais ganham.
- **Princípio do pombo (contagem)**: não existe compressor sem perdas que reduza *todas* as entradas. Entre as 2ⁿ cadeias de n bits, no máximo 2ⁿ⁻ᵏ⁺¹ − 1 podem ser comprimidas em ≥ k bits (Li & Vitányi, 2019). Para dado de entropia ~8 bit/byte (criptografado, aleatório, já comprimido), **nenhum modelo** ajuda — e o próprio README mostra `aleatorio.bin → stored, 0,0%`.

### 2.2 Complexidade de Kolmogorov

K(x) é o comprimento do menor programa que imprime x (Kolmogorov, 1965; Solomonoff, 1964; Chaitin, 1966 — referência de memória; ver Li & Vitányi, 2019). Três fatos relevantes:

1. **É incomputável**: nenhum algoritmo calcula K(x). Logo, "o melhor compressor possível" não pode ser construído, só aproximado.
2. **Teorema da invariância**: K depende da máquina de referência só até uma constante aditiva que **não depende de x** — essa constante é, na prática, **o tamanho do decodificador/modelo**.
3. Moral para o produto: dizer "armazenamos uma memória e a rede reconstrói" é equivalente a **mover bits para os pesos**. Um modelo que memorizou o corpus "comprime" o corpus a quase nada porque os bits agora estão no checkpoint. O custo honesto é sempre `|dado codificado| + |decodificador|` (princípio MDL de duas partes, Rissanen, 1978; Grünwald, 2007 — MIT Press, referência de memória).

### 2.3 Teoria taxa–distorção (lossy vs lossless)

Com critério de fidelidade d(x, x̂), a função R(D) = min I(X; X̂) sujeita a E[d] ≤ D dá a menor taxa para distorção média D (Shannon, 1959, *IRE National Convention Record* 4:142–163; Cover & Thomas, *Elements of Information Theory*, 2ª ed., Wiley, 2006, cap. 13 — de memória). Lossless é o caso D = 0.

- **Lossy** é o regime em que redes neurais já vencem codecs clássicos de forma comprovada (Seção 3): ao descartar informação imperceptível, a taxa cai muito abaixo de H.
- Blau & Michaeli (2019, ICML, arXiv:1901.07821 — de memória) mostram que, com distorção e **realismo perceptual** juntos, há um terceiro eixo: pode-se ter imagens "realistas" com distorção por pixel alta. Isso é o que modelos generativos (GAN/difusão) fazem — e é **inaceitável para auditoria**: a reconstrução pode ser plausível e **falsa**.
- **Lossless a partir de lossy** é possível (latente + resíduo): armazena-se a aproximação x̂ e o resíduo x − x̂ codificado entropicamente. O custo total é limitado inferiormente por H(X); o ganho só existe se o modelo prevê x suficientemente bem para tornar o resíduo barato.

### 2.4 "Modelagem é compressão": rede neural = modelo de probabilidade + codificador aritmético

Dada qualquer distribuição preditiva q(xₜ | x₍<ₜ₎), a codificação aritmética (Witten, Neal & Cleary, 1987; Rissanen & Langdon, 1979 — de memória) produz um código de comprimento ≈ −log₂ q(x) + ≤2 bits. Portanto:

> perda de entropia cruzada (log-loss) em bits = tamanho comprimido.

Treinar uma rede com log-loss **é** treinar um compressor; é a tese de Delétang et al. (*Language Modeling Is Compression*, ICLR 2024 — números na Seção 3) e a base do Hutter Prize / Large Text Compression Benchmark. Duas implicações operacionais, ambas decisivas:

1. **Simetria encode/decode**: o decodificador precisa reproduzir **exatamente** as mesmas probabilidades q(·) que o codificador usou, símbolo a símbolo. Uma diferença de 1 ULP num logit muda o intervalo aritmético e **destrói tudo daí em diante**. Em codecs clássicos isso é de graça (aritmética inteira/tabelas fixas); em redes neurais em float exige engenharia explícita (Seção 5).
2. **Custo por símbolo**: a decodificação roda a rede **uma vez por símbolo** (token ou byte). É por isso que compressores neurais lossless operam tipicamente em KB/s–poucos MB/s, contra centenas de MB/s do zstd (Seção 3).

### 2.5 O que a teoria **permite** e o que **proíbe** para este projeto

| A teoria **permite** | A teoria **proíbe** |
|---|---|
| Aproximar a entropia real melhor que LZ/Huffman usando um modelo mais forte, sobretudo em dados com estrutura semântica (texto, código, logs) | Comprimir **abaixo** da entropia da fonte, com qualquer modelo |
| Reconstrução bit-exata com decodificador determinístico | Reconstrução bit-exata a partir de um "latente" menor que a informação que o decodificador não consegue inferir; sem resíduo, é lossy |
| Amortizar um modelo grande sobre um corpus grande e homogêneo (o análogo do dicionário zstd, em escala) | Contar o tamanho do modelo como zero: `K` só é "constante" se o corpus for grande o bastante para diluí-lo |
| Dedup/compressão como *free lunch* quando existe redundância real | Reduzir dado de alta entropia (criptografado, aleatório, já comprimido) |
| Lossy neural agressivo, onde distorção é tolerável | Prometer "~60% menor" para qualquer tipo de arquivo, ou garantir ganho sobre os melhores compressores clássicos em todos os tipos |
| Usar redes para *prever* e *transformar* (recondicionar) antes de um codec clássico | Calcular "o melhor compressor" (K incomputável) |

**Leitura para o projeto** (inferência própria): a proposta de "memória do dado + rede que reconstrói" é teoricamente coerente **somente** como compressão com modelo de probabilidade (lossless) ou como lossy+resíduo. O ganho possível é limitado pela folga entre o modelo clássico e a entropia real **de cada tipo de dado** — grande em texto/código, moderada em imagem/áudio lossless, pequena em binários e nula em dado de alta entropia.


---

## 3. Estado da arte em compressão neural e representações implícitas

Rótulos: **[V]** número lido na fonte por pesquisador desta sessão; **[V\*]** lido via extração automática de HTML, sem conferir no PDF; **[NV]** só secundário/memória; **(deriv.)** conta própria. Razão = comprimido ÷ original (menor é melhor) salvo quando dito "razão de compressão" (maior é melhor).

### 3.1 Imagem e vídeo com perdas (lossy)

| Técnica | Ganho medido vs clássico | Custo de decodificar / codificar | Lossless? Bit-exato entre hardwares? | Fonte |
|---|---|---|---|---|
| Minnen et al. 2018 (hyperprior + autorregressivo) | −8,4% vs BPG; −59,8% vs JPEG; >−35% vs WebP/JPEG2000 | abstract sem custo; modelos com contexto serial passam de 1.000 ms/imagem no decode (ELIC) | lossy | [arXiv:1809.02736](https://arxiv.org/abs/1809.02736) [V] |
| ELIC (CVPR'22) | BD-rate **−7,88% vs VVC** (Kodak, PSNR) | decode 49,16 ms (27,8 ms versão pequena); hardware não informado | lossy | [arXiv:2203.10886](https://arxiv.org/abs/2203.10886) [V\*] |
| **JPEG AI** (ISO/IEC 6048-1:2025) | 12,5–27,9% sobre VVC intra (VM, 2024); 16,0/20,2/21,1% (decoders 0/1/2); média de **7 métricas perceptuais**, não PSNR; vence o VVC em 5 das 7 métricas contra o encoder 0 | 13/28/215 kMAC/px; decode 246–332 ms/megapixel numa V100 | **a norma diz que não é matematicamente lossless e que o modo lossless foi "deliberadamente excluído" da v1**. Bit-exatidão só exigida até o fim da decodificação aritmética (feita em **inteiros**); a síntese pode divergir "dentro de uma margem" | [JPEG press](https://jpeg.org/items/20240507_press.html), [arXiv:2510.13867](https://arxiv.org/abs/2510.13867), [ISO preview](https://assets.vde-verlag.de/iec-normen/preview-pdf/info_isoiec6048-1%7Bed1.0%7Den.pdf) [V] (texto normativo de decodificação **não lido**) |
| Decoder quantizado p/ determinismo (Koyuncu, ICASSP'24) | sem quantizar, MSE encoder×decoder RTX 4090 vs CPU i9 = 2,08×10⁴ (saída inutilizável); quantizar os **três** sub-decoders leva a erro zero | pesos/ativações 16 bits: **+0,5% BD-rate**; pesos 8 bits: **+7,9%**; GPU só acelera ≤8 bits | bit-exato **só com** quantização completa | [arXiv:2312.11209](https://arxiv.org/abs/2312.11209) [V] |
| DCVC-RT (CVPR'25), vídeo 1080p low-delay | **−21,0% de taxa vs VTM-17.0** (referência VVC); ECM-11.0 (codec clássico de pesquisa): −20,5%; DCVC-FM: −22,1% | fp16: 125 fps enc / 113 fps dec numa A100; 39,5/34,1 fps numa 2080 Ti; 385 GMAC (≈186 kMAC/px, deriv.). **VTM codifica a 0,01 fps** | versão **int16** (determinística entre dispositivos): −18,3%, mas decode 28,3 → 20,9 fps na A100 (**4–5x mais lento**). Teste cross-platform: encode A100 → decode 2080 Ti | [arXiv:2502.20762](https://arxiv.org/abs/2502.20762) [V] |
| Benchmark independente (Bristol/Visionular, 2024) | o ECM economiza **11,0% sobre o DCVC-FM**; DCVC inconsistente com movimento de fundo | — | — | [arXiv:2408.05042](https://arxiv.org/abs/2408.05042) [V] |
| MPEG/JVET NNVC | ferramentas NN dentro do codec híbrido: −11,94% luma RA vs VTM-11.0 (NNVC-4.0). **Não existe codec neural ponta a ponta padronizado** | ferramenta HOP: 466 kMAC/px (slides, jan/2025) | o próprio JVET mantém o teste **"EE1-5: reprodutibilidade bit-exata operacional"** — ainda é problema aberto | [arXiv:2309.05846](https://arxiv.org/abs/2309.05846), [relatório JVET-AQ (rascunho)](https://www.itu.int/wftp3/AV-Arch/jvet-site/2026_07_AQ_Geneva/JVET-AQ_notes_d0.docx) [V]/[NV] |

Não encontrei número direto DCVC vs AV1.

**Leitura:** o ganho neural lossy sobre o melhor codec clássico de imagem/vídeo é real (~8–28%), mas (i) é **lossy**, (ii) o próprio padrão JPEG AI abriu mão de lossless/bit-exato na v1, e (iii) o determinismo cross-hardware cobra +0,5% a +7,9% de taxa (imagem) ou 4–5x de velocidade de decode (vídeo).

### 3.2 Representações neurais implícitas (overfitting por arquivo)

| Técnica | Resultado | Custo | Fonte |
|---|---|---|---|
| COIN (2021) | MLP por imagem; supera JPEG só em baixa taxa (~0,3 bpp); perde dos codecs de ponta | encode: 50 mil iterações Adam numa 2080 Ti **por imagem** | [arXiv:2103.03123](https://arxiv.org/abs/2103.03123) [V] |
| COIN++ (2022) | CIFAR-10: ≈ BPG (1,14 bpp/28,9 dB vs 1,25 bpp/28,7 dB), "só fecha parcialmente a distância até o BPG" | encode 94,9 ms (precisa de **meta-learning prévio** no dataset); decode 1,29 ms | [arXiv:2201.12904](https://arxiv.org/abs/2201.12904) [V] |
| SIREN (2020) | representação de sinais, **não** codec; sem afirmação de compressão | — | [arXiv:2006.09661](https://arxiv.org/abs/2006.09661) [V] |
| NeRV (2021) | vídeo "comparável" a H.264/HEVC (preset medium) | codificar **é treinar**: 1.200–1.500 épocas por vídeo | [arXiv:2110.13903](https://arxiv.org/abs/2110.13903) [V] |
| C3 (2023) | iguala VTM em CLIC2020 com <3 mil MAC/px no decode | custo de encode não informado | [arXiv:2312.02753](https://arxiv.org/abs/2312.02753) [V] |
| **Cool-chic 5.0 (jul/2026)** | −11% vs VVC (VTM 28.3) em CLIC20; decode 500–3.000 MAC/px (~250x mais barato que MLIC++) | encode: 1.000 iterações ≈ **1 min numa RTX 4090 por imagem**; 10 mil iterações ≈ 10 min (−6,5% vs VVC); configs lentas até 2 h/imagem | [arXiv:2605.02726](https://arxiv.org/abs/2605.02726) [V] |

**Leitura:** INR inverte a assimetria clássica: decode barato, **encode caríssimo (minutos a horas de GPU por arquivo)**. Para I/O de data center (escrita contínua de bilhões de objetos) isso é inviável; só faria sentido para conteúdo escrito uma vez e lido milhões de vezes (distribuição de mídia), e é lossy. Determinismo de INR: **não encontrei fonte**; como o decode é uma passada em ponto flutuante, vale o mesmo problema da Seção 5 (inf.).

### 3.3 Lossless neural de imagem e áudio

Bits por subpixel (bpsp), menor é melhor; DLPR = Deep Lossy Plus Residual Coding (TPAMI) [V]:

| Codec | ImageNet64 | DIV2K | CLIC.p | CLIC.m | Kodak |
|---|---:|---:|---:|---:|---:|
| PNG | 5,42 | 4,23 | 3,93 | 3,93 | 4,35 |
| WebP | 4,36 | 3,11 | 2,90 | 2,73 | 3,18 |
| FLIF | 4,25 | 2,91 | 2,72 | 2,48 | 2,90 |
| **JPEG-XL** | 4,94 | 2,79 | 2,63 | 2,36 | 2,87 |
| L3C (2019) | 4,48 | 3,09 | 2,94 | 2,64 | 3,26 |
| iVPF | 3,75 | 2,68 | 2,54 | 2,39 | — |
| **DLPR** | 3,69 | 2,55 | 2,38 | 2,16 | 2,86 |

- DLPR vs JPEG-XL: **−0,01 bpsp em Kodak** (2,86 vs 2,87), −0,24 em DIV2K, −0,25 em CLIC.p. Tempo (768×512, enc/dec): JPEG-XL 0,73/0,08 s; FLIF 0,90/0,16 s; L3C 8,17/7,89 s; DLPR 1,26/1,80 s. Hardware não registrado. ([arXiv:2209.04847](https://arxiv.org/abs/2209.04847))
- **L3C não bate o FLIF** em Open Images (2,991 vs 2,867 bpsp) e seu decode (0,374 s) é mais lento que o do FLIF (0,133 s). ([arXiv:1811.12817](https://arxiv.org/abs/1811.12817))
- IDF/iVPF: iVPF é numericamente invertível (sem erro de ponto flutuante): CIFAR10 3,20 bpd vs FLIF 4,37 no mesmo paper do IDF (3,34). ([IDF](https://arxiv.org/abs/1905.07376), [iVPF](https://arxiv.org/abs/2103.16211)). BB-ANS: MNIST 1,41 bpd vs bz2 1,42 ([arXiv:1901.04866](https://arxiv.org/abs/1901.04866)); código Python em CPU descrito como gargalo, sem discutir determinismo.
- LLM como compressor de imagem (Delétang 2024): Chinchilla 70B comprime ImageNet a 48,0% (Tabela 1) vs PNG 58,5% ([arXiv:2309.10668](https://arxiv.org/abs/2309.10668)). **Aviso:** o abstract do mesmo PDF diz 43,4% (ImageNet) e 16,4% (LibriSpeech), a Tabela 1 diz 48,0% e 21,0%; o pesquisador não conseguiu reconciliar. Uso a Tabela 1 (conservadora).
- **LMCompress** (Li et al., *Nature Machine Intelligence* 7:794–799, 2025): razões de compressão ILSVRC 4,79 vs PNG 1,67/JPEG-XL 1,90; CLIC2019 6,32 vs 2,93; LibriSpeech 6,07 vs FLAC 3,23. **Desconfiança:** a magnitude em ImageNet destoa de todos os outros trabalhos da tabela acima (DLPR em ImageNet64 ≈ razão 2,2); o paper **não informa hardware, velocidade, determinismo nem se o modelo é contado**. Não usar como evidência de viabilidade. ([arXiv:2407.07723](https://arxiv.org/abs/2407.07723), DOI [10.1038/s42256-025-01033-7](https://doi.org/10.1038/s42256-025-01033-7); versão Nature não aberta)
- Áudio lossless (Long et al., Interspeech 2026): modelos de linguagem 16 bits: VCTK 2,66 vs FLAC-8 2,32; LibriSpeech 2,11 vs 1,74 (+18% médio); 8 bits: +217%; **24 bits: FLAC vence (1,63 vs 1,48)**. São **estimativas pela perda do modelo, não arquivos reais**, "ordens de grandeza mais lentos que o FLAC". Não achei modelo lossless de áudio com bitstream real e velocidade medida. ([arXiv:2603.08683](https://arxiv.org/abs/2603.08683))
- Codecs de áudio neurais (SoundStream: 3 kbps > Opus 12 kbps; EnCodec: 3 kbps > Lyra-v2 6 kbps e Opus 12 kbps, em MUSHRA; DAC ~90x a 8 kbps) são **todos lossy**, treinados com perda adversarial; nenhum é bit-exato.

### 3.4 Texto e dados genéricos: modelo de linguagem + codificador aritmético

**Benchmark de referência (enwik9 = 10⁹ bytes de Wikipedia; LTCB, Mahoney)** [V]:

| Compressor | Tamanho (bytes) | Razão | Compressão | Descompressão | Notas |
|---|---:|---:|---:|---:|---|
| **cmix v21** | 107.963.380 | 10,8% | ~1,6 KB/s | ~1,6 KB/s | 31 GB RAM; i7-7700K; simétrico |
| **NNCP v3.2** | 106.632.363 | 10,66% | ~4,1 KB/s | ~4,2 KB/s | RTX 3090 + Xeon; decoder **retreina** o modelo online |
| paq8px v206 -12L | 124.696.410 | 12,5% | ~3,4 KB/s | — | 28 GB RAM |
| **ts_zip** (RWKV 169M, 8 bits) | 135.443.237 | 13,5% | **até ~1 MB/s** | **até ~1 MB/s** | RTX 4090; modelo (~169 MB) **não contado**; "experimental, sem compatibilidade retroativa" |
| **xz -9e** | 197.331.816 | 19,7% | ~170 KB/s | ~50 MB/s | |
| **zstd -22** | 215.674.670 | 21,6% | ~1,4 MB/s | ~450 MB/s | zstd 0.6.0 (antigo) |
| brotli -q11 | 223.597.884 | 22,4% | — | — | |
| bzip2 -9 | 253.977.839 | 25,4% | — | — | |

Fontes: [LTCB](https://www.mattmahoney.net/dc/text.html), [NNCP/ts_zip](https://bellard.org/nncp/), [ts_zip](https://bellard.org/ts_zip/), [cmix](http://www.byronknoll.com/cmix.html). Tempos de linhas diferentes foram medidos em hardwares diferentes (Mahoney avisa que variam muito). Velocidades em KB/s são derivadas de ns/byte (deriv.). Referência clássica (zstd 1.5.7, Silesia, i7-9700K, lzbench): zstd -1 razão 2,896 a **510 MB/s** compressão e **1.550 MB/s** descompressão ([README zstd](https://github.com/facebook/zstd), V).

Entradas de 2026 no mesmo LTCB (relatadas pelo pesquisador; **a página indexada pela busca aparece "atualizada em 25/03/2026", então estas linhas não foram reconfirmadas por mim**): cmix-neif 1.0 chega a 91,9 MB com Transformer de 6M parâmetros em 4 bits, só CPU, ~48,7 h por GB; tufazip v2 usa 7,5 dias em 2×B200. São resultados de terceiros "não verificados" por Mahoney e otimizados para enwik9.

**Compressores com LLM — razão e velocidade:**

| Trabalho | Resultado | Velocidade | Modelo contado? | Fonte |
|---|---|---|---|---|
| Delétang et al. (ICLR'24) | enwik9: Chinchilla 70B **8,3%**, Llama2-7B 8,9%; gzip 32,3%, LZMA2 23,0% (sem chunk) | não reportada | **Taxa ajustada (soma o modelo): Chinchilla 70B 14.008%**, Llama2-7B 1.409%, Chinchilla 1B 211%. Só Transformers pequenos treinados no enwik8 ficam <100% (3,2M parâmetros: 17,7%) | [arXiv:2309.10668](https://arxiv.org/abs/2309.10668) [V] |
| LLMZip (2023) | LLaMA-7B: 0,7101 bpc em text8 (contaminação provável) | ~12 B/s (deriv. de FineZip) | não | [arXiv:2306.04050](https://arxiv.org/abs/2306.04050) [V] |
| FineZip (2024) | 10 MB do enwik8, A6000, Llama-3 8B+LoRA: razão 0,0797 vs bzip2 0,2374, NNCP 0,150 | **~0,67 KB/s** (versão 4 bits: ~2,5 KB/s); só compressão medida | não | [arXiv:2409.17141](https://arxiv.org/abs/2409.17141) [V\*] |
| Heurtel-Depeiges et al. | Transformer ~20M, razão **ajustada** em texto Reddit fora de distribuição: 0,217 vs LZMA2 0,286 e Bellard online 0,204; áudio 0,487 vs FLAC 0,538 | ~2,2 KB/s (vs LZMA2 ~1,1 MB/s) para 1 GB; decode "ainda pior" | **sim** | [arXiv:2410.05078](https://arxiv.org/abs/2410.05078) [V] |
| **L3TC (AAAI'25)** | RWKV 3,2M: **16,87% ajustado** vs zstd -22 21,62%, bzip2 25,40%, gzip 32,26% | decode **4,35 MB/s** (variante 200K, A100); ts_zip 180 KB/s; RWKV-7B 4 KB/s; Llama2-7B 280 B/s; Llama2-13B 85 B/s | **sim** | [arXiv:2412.16642](https://arxiv.org/abs/2412.16642) [V\*] |
| PMATIC (ICLR'26) | enwik8 ~10 MB: LLaMA-3.1-8B 4 bits 0,0780 vs cmix 0,3558, brotli-11 0,3524, xz 0,4647 | — | não; contaminação provável | [arXiv:2601.10678](https://arxiv.org/abs/2601.10678) [V] |
| Hybrid-LLM (2026) | Llama-3 8B, RTX 3090: alice29 0,39 bpc (texto provavelmente memorizado) vs gzip 2,70 | ~115 B/s (**~2.600x mais lento que zstd**) | não | [arXiv:2603.25526](https://arxiv.org/abs/2603.25526) [V\*] |
| Nacrith (fev/2026, preprint de autor único) | SmolLM2-135M: enwik8 0,939 bpb vs xz 1,989; alice29 0,918 | ~20–30 tokens/s numa GTX 1050 Ti | modelo ~500 MB fora do resultado; **contaminação provável** (o próprio autor admite) | [arXiv:2602.19626](https://arxiv.org/abs/2602.19626) [V] (existência conferida por mim) |
| Ferragina & Nardone (jul/2026), código-fonte | LLM com *symbol ranking* + bzip3/zstd: até **82% de ganho relativo** sobre zstd/bzip2 | modelos mais rápidos ≤ ~0,6 MB/s (8×H100), maioria ≤0,1 MB/s; só compressão medida | não informado | [arXiv:2607.24192](https://arxiv.org/abs/2607.24192) [V] |

**Três conclusões que a evidência sustenta:**

1. **Os ganhos grandes (8–11%) dependem de não contar o modelo e de contaminação de treino.** Chinchilla foi treinado em Wikipédia e enwik9 é Wikipédia; vários resultados de 2026 admitem o mesmo. Quando o modelo é contado e o dado é fora de distribuição, o ganho sobre `zstd -22`/LZMA2 cai para **~3–8 pontos percentuais** (L3TC: 16,87% vs 21,62%; Heurtel-Depeiges: 0,217 vs 0,286).
2. **O modelo pré-treinado grande só se paga com corpus na ordem de TB** (Delétang, Seção 3.3 do paper). Condição de amortização (deriv.): D_min = M/Δ, com M = tamanho do modelo e Δ = ganho fracionário sobre o melhor clássico. ts_zip: 0,169 GB / 0,062 ≈ **2,7 GB**; LLM 7B fp16: 14 / 0,11 ≈ **127 GB**; Chinchilla 70B fp16: 140 / 0,114 ≈ **1,2 TB** — por cópia do modelo mantida em cada nó de decodificação.
3. **A velocidade é o limite duro:** do melhor caso prático (ts_zip, ~1 MB/s em GPU de topo; L3TC 200K, 4,35 MB/s com razão pior) até 85 B/s (Llama2-13B). Compare com zstd -1: 510 MB/s por núcleo.

### 3.5 Binários, dados estruturados e logs

| Fato | Fonte | Conf. |
|---|---|---|
| **Executável** `mozilla` (51,2 MB), bits por caractere: gzip 2,97; 7zip 2,11; ZPAQ 1,88; NNCP 1,63; cmix 1,39; **DZip 2,15 (perde para o 7zip)**, com modelo ~25% do arquivo. Floats: cmix 1,58 vs DZip 2,18 vs FPC 7,74 (`obs-spitzer`) | DZip, [arXiv:1911.03572](https://arxiv.org/abs/1911.03572) | V |
| FADE (ACL'26), compressor aprendido **online**, RTX 4090: Silesia razão 5,400 vs 7z 4,352 e zstd 4,008 (+24% sobre 7z) a ~76/69 KB/s (comp/decomp); floats climáticos CESM 2,939 vs 7z 1,829 vs zstd 1,404. cmix no mesmo Silesia: 7,50 (deriv.) | [arXiv:2604.07239](https://arxiv.org/abs/2604.07239) | V |
| TRACE (WWW'22), RTX 2080: gzip 1.851 MB/min; 7z 1.032 MB/min; zstd -19 191 MB/min; cmix 39,9 kB/min; DZip 399 kB/min; TRACE 952 kB/min | [arXiv:2203.16114](https://arxiv.org/abs/2203.16114) | V |
| **Logs, tabelas SQL e séries temporais:** nenhum benchmark neural direto contra zstd/xz encontrado | — | **não encontrei** |

### 3.6 Modelos generativos (difusão/GAN): por que são inadequados à auditoria

- **Rate–distortion–perception** (Blau & Michaeli, ICML'19): exigir qualidade perceptual alta eleva a curva taxa–distorção; baixa distorção ≠ alta qualidade perceptual ([arXiv:1901.07821](https://arxiv.org/abs/1901.07821), V).
- HiFiC: usuários o preferem mesmo contra codecs com mais de 2x o bitrate — preferência **perceptual**, não fidelidade ao pixel ([arXiv:2006.09965](https://arxiv.org/abs/2006.09965)). DiffC (Theis 2022) supera o HiFiC em ImageNet 64×64 ([arXiv:2206.08889](https://arxiv.org/abs/2206.08889)).
- **"Miscompression":** Hofer & Böhme (WIFS 2024) formalizam a alteração semântica que parece perfeita e propõem taxonomia com marcação de "alto impacto" ([arXiv:2409.05490](https://arxiv.org/abs/2409.05490), V). Caso real análogo, não neural: o JBIG2 da Xerox WorkCentre trocava dígitos (6↔8, 65↔85) por 8 anos com cópias de aparência correta ([Kriesel, 2013](https://www.dkriesel.com/en/blog/2013/0802_xerox-workcentres_are_switching_written_numbers_when_scanning), V).
- Contra-exemplo (só snippet, **[NV]**): Cardenuto et al. (IH&MMSEC 2025) acham que compressão neural preservou traços forenses melhor que JPEG a bitrate parecido.
- **Único caso real de armazenamento "latent-first"** (LatentStore, arXiv:2605.19385 v3, out/2026, V): funciona **somente** onde a imagem *é* a saída do decoder VAE (imagens geradas por IA: SDXL, SD 3.5, FLUX…). −78,7% de armazenamento vs PNG num trace de 35 meses e ~2 bilhões de requisições; latente comprimido 43–321 KB vs PNG 1,3–1,6 MB; decode 32,6 ms (H100) a 67,2 ms (RTX 4090). **Entre H100 e L4 só 47% dos valores pixel-canal ficam bit-exatos** (o resto muda ±1 a 3 em 255 níveis); mitigação: etiquetar cada latente com (ID do modelo, versão do decoder, perfil de precisão) e arquivar os pesos do decoder (<100 MB por família). **Não se estende a dados reais** (câmera, áudio, logs). ([arXiv:2605.19385](https://arxiv.org/abs/2605.19385))

Isto é a resposta mais direta à ideia "armazenar uma memória e reconstruir": ela **funciona de verdade** quando o dado foi *gerado* pelo mesmo modelo que o reconstrói; para dado capturado do mundo, o latente vem de um encoder e a reconstrução é lossy.

### 3.7 Deduplicação semântica, hashing aprendido e redução de tokens

- SemDeDup (Abbas et al. 2023): remove ~50% de um subconjunto do LAION com perda mínima no treino; "duplicados semânticos" = "semanticamente similares, mas não idênticos" — é **redução de dataset, não armazenamento reversível** ([arXiv:2303.09540](https://arxiv.org/abs/2303.09540), V). Lee et al. 2022: dedup exato e aproximado de dados de treino ([arXiv:2107.06499](https://arxiv.org/abs/2107.06499), V abstract).
- MinHash/shingling (Broder 1997) e SimHash (Charikar 2002) estimam similaridade (Jaccard/ângulo); o limiar de "quase duplicado" é difuso (Broder: V via resumo; Charikar e *perceptual hashing*/*learned hashing*: **NV**).
- **Para produto bit-exato:** embeddings, MinHash e SimHash decidem "parecido", nunca "idêntico"; descartar um dos arquivos perde bytes. Bit-exato exige hash criptográfico do conteúdo (dedup exato) ou *chunking* por conteúdo (FastCDC), com referências ou **deltas bit-exatos** contra uma base (isso é compressão por delta, não dedup semântico — inferência do pesquisador, que adoto).
- Redução de tokens (contexto): LLMLingua corta prompts em até 20x com pouca perda de desempenho ([arXiv:2310.05736](https://arxiv.org/abs/2310.05736), V); compressão de KV-cache é outra família igualmente lossy (NV). Nenhuma é reversível. Não aprofundo: é um problema diferente de armazenamento (Seção 1, P10).

### 3.8 Síntese comparativa

| Abordagem | Ganho sobre o melhor clássico (modelo contado) | Vazão típica | Lossless | Bit-exato entre hardwares sem custo extra |
|---|---|---|:-:|:-:|
| Codecs neurais de imagem/vídeo | −8% a −28% de taxa | 0,03–125 fps | ✗ | ✗ (+0,5–7,9% de taxa ou 4–5x mais lento) |
| INR por arquivo | ≈ H.264/BPG à custa de encode de minutos–horas/arquivo | decode barato | ✗ | não verificado |
| Lossless neural de imagem | 0,01–0,25 bpsp sobre JPEG-XL | decode 1,8 s vs 0,08 s (imagem 768×512) | ✓ | não discutido nos papers |
| LM + aritmético em texto | 3–8 p.p. sobre zstd/xz (modelo contado, fora de distribuição); 6–11 p.p. com modelo não contado | **0,0001 – 4 MB/s** | ✓ (se determinístico) | ✗ medido: 0 de 150 arquivos (PMATIC) |
| Compressor online aprendido (FADE/NNCP/cmix) | até +24% sobre 7z (FADE), cmix +72% em Silesia | 1,6 KB/s – 76 KB/s | ✓ | CPU/NC: sim; GPU: não |
| Generativo (difusão/GAN) | maior ganho perceptual | — | ✗ | ✗ |
| Latent-first (LatentStore) | −78,7% vs PNG | decode 33–67 ms | ✓ no latente | ✗ (47% bit-exato entre H100 e L4) |
| Dedup semântica/embeddings | n/a (descarta bytes) | — | ✗ | — |


---

## 4. Estado da arte em armazenamento e data centers

Legenda de confiança usada em todo o documento: **[V]** = fonte aberta e número lido por um pesquisador desta sessão; **[S]** = agregador/imprensa/resumo, fonte primária não conferida; **[NV]** = só trecho de busca ou memória; **(deriv.)** = conta feita aqui a partir de números citados. Preços são de **2026-11-03** salvo indicação.

### 4.1 Deduplicação e compressão transparente

| Sistema / estudo | Fato | Fonte | Conf. |
|---|---|---|---|
| ZFS dedup | Doc OpenZFS recomenda **≥1,25 GiB de RAM por TiB** com dedup, e sugere compressão como alternativa mais barata. DDT com ~300–900 B/entrada; TrueNAS só recomenda dedup com razão ≥3x. | [OpenZFS](https://openzfs.github.io/openzfs-docs/man/master/7/zfsconcepts.7.html), [TrueNAS](https://truenas.com/docs/references/zfsdeduplication) | V |
| ZFS dedup (conta) | 1 TiB em blocos de 128 KiB ≈ 2,7 GB de DDT; em blocos de 8 KiB ≈ 43 GB (assumindo 320 B/entrada) | — | deriv. |
| ZFS compressão | `compression=on` → lz4; também zstd-1..19, zstd-fast, gzip-1..9 | [OpenZFS zfsprops](https://openzfs.github.io/openzfs-docs/man/master/7/zfsprops.7.html) | V |
| zstd/lz4 (Silesia, i7-9700K) | lz4: razão 2,101, 675 MB/s comp., 3850 MB/s descomp.; zstd -1: 2,896, 510 / 1550 MB/s; zlib -1: 2,743, 105 / 390 MB/s | [README zstd](https://github.com/facebook/zstd) | V |
| Ceph BlueStore | Compressão **desligada por padrão** (modo `none`); algoritmos snappy/zlib/lz4/zstd; `required_ratio` 0,875; doc "não recomenda" zstd por CPU em blocos pequenos | [Ceph docs](https://docs.ceph.com/en/latest/rados/configuration/bluestore-config-ref/) | V |
| Ceph dedup | Experimental sobre RADOS; fingerprint sha1/256/512; chunk fixo ou CDC | [Ceph dev](https://docs.ceph.com/en/latest/dev/deduplication/) | NV |
| Data Domain (Zhu, Li, Patterson, FAST'08) | Bloom "Summary Vector" + SISL + cache por localidade eliminam ~99% dos acessos ao índice em disco; **100 MB/s** (1 stream) e **210 MB/s** (multi) com 1 shelf de 15 discos | [PDF](https://www.usenix.org/legacy/event/fast08/tech/full_papers/zhu/zhu.pdf) | V |
| FastCDC (Xia et al., ATC'16) | ~10x mais rápido que Rabin CDC open-source, ~3x mais rápido que Gear/AE, com razão de dedup quase igual | [PDF](https://www.usenix.org/system/files/conference/atc16/atc16-paper-xia.pdf) | V |
| Meyer & Bolosky, FAST'11 | 857 desktops, 162 TB: dedup por bloco leva o consumo a até **32%** (~3,1x); dedup de arquivo inteiro captura ~3/4 da economia em FS vivos e 87% em backups | [PDF](http://www.usenix.org/events/fast11/tech/full_papers/Meyer.pdf) | V |
| Wallace et al., FAST'12 | >10 mil sistemas Data Domain: dedup de backup **mediana 8,7x, média 10,9x**; compressão local pós-dedup média ~1,9x (mediana 1,7x); carga primária ~3x | [PDF](https://www.usenix.org/system/files/conference/fast12/wallace.pdf) | V |

**Leitura para o projeto:** a economia robusta e comprovada em escala de produção vem de **dedup por bloco + compressão clássica**, não de modelagem. Mesmo em storage de nível empresarial, ~3x é típico em dado primário e ~9–11x em backup (muita repetição entre versões). O Syntra só faz dedup por arquivo inteiro (P8 da Seção 1): o dado de Meyer & Bolosky indica que ativar CDC é a melhoria de maior retorno/menor risco do roadmap, antes de qualquer rede neural (inferência própria).

### 4.2 Erasure coding vs replicação

| Esquema | Overhead | Fonte | Conf. |
|---|---|---|---|
| Replicação tripla | 3,0x | Huang et al., ATC'12 | V |
| Reed-Solomon (6,3) | 1,5x | idem | V |
| LRC (12,2,2) (Azure) | **1,33x**, durabilidade superior a 3 réplicas; ~metade do footprint e energia de 3 cópias num sistema >1 EB | [PDF](https://www.usenix.org/system/files/conference/atc12/atc12-final181_0.pdf) | V |
| Backblaze Vault RS 17+3 | 20/17 = **1,176x**; 20 pods × 60 discos; tolera 3 discos/tome; 11 noves de durabilidade (modelo de Poisson, AFR efetivo 0,41%, rebuild 6,5 dias) | [Vault](https://www.backblaze.com/blog/vault-cloud-storage-architecture/), [durabilidade](https://www.backblaze.com/blog/cloud-storage-durability/) | V |

Consequência: o piso de redundância física é ~1,2–1,5x. Uma compressão de 60% **não** reduz o custo físico em 60%: reduz o dado, mas a redundância continua multiplicando o que sobrou. Vale igual para clássico e neural (inferência própria).

### 4.3 Hierarquia de custo por byte (2026-11-03)

| Camada | Preço | Obs. | Conf. |
|---|---|---|---|
| DRAM DDR5 RDIMM 64 GB ECC | ~US$35/GB (US$2.245/módulo), preço de rua | contrato: TrendForce 1T26 **+90–95% QoQ**; 3T26 +13–18%; 4T26 +10–15% | V (agregador) / V |
| SSD NVMe enterprise (Solidigm D5-P5316 15,36 TB) | ~US$117/TB (mais barato rastreado, ago/2026) | NAND de contrato: 1T26 +55–60% QoQ; 3T26 +10–15%; 4T26 +15–20%. Acumulado 2026 ≈ **3,6x** (pontos médios, fontes misturadas, ordem de grandeza) | S / V / deriv. |
| HDD | US$28–41/TB (piso Amazon 20–24 TB, out/2026); Exos SATA US$27,92/TB | WD: ASP/TB +28% a/a (jul–set/2026). NAND subiu muito mais que HDD nearline | S |
| Fita LTO-9 (18 TB) | ~US$5,50/TB | LTO-10 30 TB nativo, 400 MB/s; cartucho ~US$300 (~US$10/TB, deriv.) | V (agregador) / NV |
| S3 Standard | US$0,023/GB-mês | [JSON de preços AWS](https://b0.p.awsstatic.com/pricing/2.0/meteredUnitMaps/s3/USD/current/s3.json) | V |
| S3 Glacier Instant Retrieval | US$0,004/GB-mês; retrieval US$0,03/GB; mínimo 90 dias | idem | V |
| S3 Glacier Flexible | US$0,0036/GB-mês; mínimo 90 dias | idem | V |
| S3 Glacier Deep Archive | US$0,00099/GB-mês; retrieval US$0,02/GB (std); mínimo 180 dias | valor lido na camada Deep Archive do Intelligent-Tiering (rotulagem não explícita) | V/ressalva |
| Egress AWS | US$0,09/GB (primeiros 10 TB) → US$0,05/GB (>150 TB) | [datatransfer.json](https://b0.p.awsstatic.com/pricing/2.0/meteredUnitMaps/datatransfer/USD/current/datatransfer.json) | V |
| Backblaze B2 | a partir de US$6,95/TB-mês; egress grátis até 3x o armazenado, depois US$0,01/GB | [B2](https://www.backblaze.com/cloud-storage/pricing) (página sem data) | V |
| Cloudflare R2 | Standard US$0,015/GB-mês; IA US$0,01 + US$0,01/GB retrieval; egress grátis | [R2](https://developers.cloudflare.com/r2/pricing/) (sem data) | V |

**Alerta de preço (importante):** NAND e DRAM tiveram alta anormal em 2026 (TrendForce). O ganho de comprimir tende a ser **maior** em SSD/DRAM do que em HDD/fita; o gargalo do raciocínio "armazenar é barato" vale para HDD, fita e objeto frio, não para flash.

Âncora para o requisito de **5 TB** (deriv.): custo mensal de 5 TB = US$115 (S3 Standard), US$34,75 (B2), ~US$140–205 de discos brutos pagos uma vez. Uma redução de 60% economiza algumas centenas de dólares por ano nessa escala — a economia relevante aparece em **centenas de TB a PB** (Seção 6).

### 4.4 Custo e potência de computação

| Item | Valor | Fonte | Conf. |
|---|---|---|---|
| AWS p5.48xlarge (8×H100) | US$55,04/h on-demand (US$6,88/GPU-h); spot US$32,49/h (US$4,06/GPU-h) | [Vantage](https://instances.vantage.sh/aws/ec2/p5.48xlarge) | V (agregador) |
| AWS p4d.24xlarge (8×A100) | US$21,96/h (US$2,74/GPU-h); spot US$18,31/h | Vantage | V (agregador) |
| AWS g6.xlarge (1×L4) | US$0,805/h; spot US$0,594/h | Vantage | V (agregador) |
| AWS g4dn.xlarge (1×T4) | US$0,526/h; spot US$0,264/h | Vantage | V (agregador) |
| AWS c7i.xlarge (4 vCPU) | US$0,1785/h (US$0,0446/vCPU-h); spot US$0,067/h | Vantage | V (agregador) |
| Lambda on-demand | H100 SXM US$3,99–4,29/GPU-h; A100 80 GB US$2,79; A100 40 GB US$1,99; B200 US$6,69–6,99 | [Lambda](https://lambda.ai/pricing) (sem data) | V |
| TDP | L4 72 W; T4 70 W; H100 SXM até 700 W; H100 PCIe 350–400 W; A100 SXM 400 W / PCIe 300 W | páginas NVIDIA | V |

Spot do Google/Azure: **não encontrado**.

### 4.5 Energia: computar vs armazenar

| Item | Valor | Fonte | Conf. |
|---|---|---|---|
| HDD 26 TB (WD HC590) | idle 5,6 W; 4 KB aleatório 50/50 8,4 W; 302 MB/s; AFR 0,35% | [datasheet](https://documents.westerndigital.com/content/dam/doc-library/en_us/assets/public/western-digital/product/data-center-drives/ultrastar-dc-hc500-series/data-sheet-ultrastar-dc-hc590.pdf) | V |
| HDD 24 TB (Seagate Exos X24) | idle ~6,3 W (SATA); 285 MB/s; AFR 0,35% | [datasheet](https://www.seagate.com/content/dam/seagate/en/content-fragments/products/datasheets/exos-x24/exos-x24-DS2080-2307US-en_US.pdf) | V |
| SSD 61,44 TB (Solidigm D5-P5336) | máx. 25 W; idle <5 W; 7.000 MB/s leitura seq. | [brief](https://www.solidigm.com/content/dam/solidigm/en/site/products/technology/p5336-product-brief/documents/solidigm-d5-p5336-product-brief.pdf) | V |
| **Energia de armazenar (deriv., 8.760 h, sem PUE)** | HDD 26 TB idle **1,89 kWh/TB-ano**; ativo 2,83; SSD 61 TB idle 0,71; 25 W 3,56; fita (CERN, biblioteca inteira) ~0,13 | — | deriv. |
| Fita no CERN | ~18 kW para ~1,2 EB (~15 W/PB); 0,25 t CO₂e/PB-ano | [poster SC4RC 2026](https://indico.cern.ch/event/1526482/contributions/7027775/attachments/3267151/5835421/Environmental%20efficiency%20of%20tape%20storage%20at%20CERN%20-%20SC4RC%202026%20-%20poster.pdf) | V |
| Carbono embutido do flash | ~0,16 kg CO₂/GB fabricado; ~20 Mt CO₂ em 2021 (Tannu & Nair). **Contestado em 2025** por Pure, Ocient e Solidigm | [arXiv:2207.10793](https://arxiv.org/abs/2207.10793) | V (abstract) |
| PUE médio (Uptime 2026) | 1,52 (ponderado por capacidade 1,36) | [Uptime](https://intelligence.uptimeinstitute.com/resource/growing-pue-advantage-larger-data-centers) | V |
| Data centers EUA (LBNL, dez/2024) | 176 TWh em 2023 (4,4%); 325–580 TWh em 2028 | [DOE](https://www.energy.gov/articles/doe-releases-new-report-evaluating-increase-electricity-demand-data-centers) | V |
| IEA "Energy and AI" (abr/2025) | ~945 TWh em 2030 (≈415 TWh em 2024) | [IEA](https://www.iea.org/reports/energy-and-ai) | NV (415) |
| Inferência de LLM em escala (Google) | mediana 0,24 Wh por prompt de texto (Gemini Apps) | [arXiv:2508.15734](https://arxiv.org/abs/2508.15734) | V (abstract) |

**Comparação decisiva (deriv.):** armazenar 1 TB em HDD custa ~1,9–2,8 kWh/ano. Reconstruir 1 TB com um compressor neural a 1 MB/s em GPU de 72 W custa ~**20 kWh por passagem** (72 W ÷ 3,6 GB/h), ou seja, **7–10x a energia de um ano inteiro de armazenamento do mesmo terabyte**, a cada leitura (72 W é TDP, não medição). Para o `zstd`, a energia de descompressão é desprezível. A Seção 6 fecha o balanço de custo.

Não encontrado: estudo Seagate/IDC de Wh/TB; participação do armazenamento no consumo total de data centers (IEA/LBNL); potência de drive LTO-9/10.

### 4.6 Dados de campo de confiabilidade (Backblaze, Drive Stats 2025)

AFR da frota **1,36%** em 2025 (1,55% em 2024), 344.196 discos, 30 modelos ([Backblaze, fev/2026](https://www.backblaze.com/blog/backblaze-drive-stats-for-2025/), V). Por modelo (transcrição do StorageReview, **[S]**): WDC 22 TB 0,47%; Toshiba MG10 20 TB 0,59%; Toshiba MG11 24 TB 0,68%; Seagate ST24000NM002H 24 TB **2,75%**; Toshiba MG08 16 TB **6,30%** (pior do ano). Há variação de >10x entre modelos: a taxa de falha é propriedade do modelo/lote, não do "HDD" genérico.

### 4.7 Latência e banda

HDD 285–302 MB/s sustentado, ~4,2 ms médio; NVMe PCIe 4.0 até 7.000 MB/s (leitura seq.); LTO-10 400 MB/s nativos; S3 primeiro byte ~100–200 ms (objetos pequenos), Glacier Flexible leva minutos a horas, Deep Archive horas ([AWS](https://docs.aws.amazon.com/AmazonS3/latest/userguide/optimizing-performance.html), V). Egress: S3 US$0,09/GB, B2 grátis até 3x, R2 grátis.

**Hipótese de trabalho (não é dado):** decodificação neural genérica a ~0,1–4 MB/s por GPU (medidos na Seção 3 para texto). Frente aos 285–7.000 MB/s do meio físico, o decodificador é o gargalo por 2–4 ordens de grandeza.


---

## 5. Modos de falha e corrupção

Rótulos: **[V]** lido na fonte primária por pesquisador desta sessão; **[NV]** só secundário/resumo; **(deriv.)** conta ou simulação própria, **não revisada**; **(inf.)** inferência própria.

### 5.1 Evidência por risco

**(a) Corrupção silenciosa em storage**

| Fato | Fonte | Conf. |
|---|---|---|
| 1,53 milhão de discos, 41 meses: >400 mil *checksum mismatches*; 0,86% dos discos nearline e 0,065% dos enterprise tiveram ≥1; erros com forte localidade espacial/temporal (não independentes); 8% foram descobertos durante reconstrução RAID | Bairavasundaram et al., FAST'08 ([PDF](https://www.usenix.org/legacy/event/fast08/tech/full_papers/bairavasundaram/bairavasundaram.pdf)) | V |
| 3,45% dos discos desenvolveram *latent sector errors* em 32 meses (~8,5% nearline, ~1,9% enterprise); *scrubbing* descobre >60% | Bairavasundaram et al., SIGMETRICS'07 ([PDF](https://pages.cs.wisc.edu/~remzi/Classes/739/Fall2015/Papers/latent-sigmetrics07.pdf)) | V |
| Taxa anual de reposição tipicamente >1%, 2–4% comum, até 13% em alguns sistemas (datasheet sugere ≤0,88%) | Schroeder & Gibson, FAST'07 ([PDF](https://www.cs.utoronto.ca/~bianca/papers/fast07.pdf)) | V |
| AFR de 1,7% no 1º ano a >8,6% em discos de 3 anos; SMART isolado "improvável de ser útil" para prever falha individual | Pinheiro et al., FAST'07 ([Google](https://research.google/pubs/failure-trends-in-a-large-disk-drive-population/)) | V |
| Flash em produção (6 anos): 20–90% dos drives com ≥1 *uncorrectable error*; UBER "não é uma métrica significativa" (sem correlação com nº de leituras) | Schroeder et al., FAST'16 ([USENIX](https://www.usenix.org/conference/fast16/technical-sessions/presentation/schroeder)) | V |
| ZFS por injeção de falhas: robusto a falhas de disco, **muito menos a corrupção de memória**; checksum em disco não substitui ECC | Zhang et al., FAST'10 ([PDF](https://www.usenix.org/legacy/event/fast10/tech/full_papers/zhang.pdf)) | V |
| Spec de UBER em datasheets: Exos "1 setor por 10¹⁵"; WD Red Plus "<1 em 10¹⁴"; Samsung PM1643 "1 setor por 10¹⁷ bits" | datasheets Seagate/WD/Samsung | V |
| DRAM: 25–70 mil erros/bilhão de device-horas/Mbit; >8% dos DIMMs com erro corrigível por ano; **1,3% das máquinas com erro não corrigível por ano** (até 2–4% em algumas plataformas) | Schroeder et al., SIGMETRICS'09 ([PDF](https://www.cs.toronto.edu/~bianca/papers/sigmetrics09.pdf)) | V |

*Conta para 5 TB (deriv., assume erros independentes — premissa que o próprio FAST'08 contradiz — e usa o limite do spec como taxa):* 5 TB = 4×10¹³ bits.

| UBER do spec | Erros esperados por leitura completa | P(≥1 erro) |
|---|---:|---:|
| 10⁻¹⁴ | 0,4 | 33% |
| 10⁻¹⁵ | 0,04 | 3,9% |
| 10⁻¹⁶ | 0,004 | 0,4% |
| 10⁻¹⁷ | 0,0004 | 0,04% |

O UBER mede erros **reportados** (detectáveis); a corrupção silenciosa é outra categoria. Para flash, o FAST'16 diz que UBER nem é bom preditor. A tabela é ordem de grandeza, não previsão.

**(b) Silent Data Corruption em CPU/GPU**

| Fato | Fonte | Conf. |
|---|---|---|
| "Mercurial cores": ~"a few per several thousand machines"; afetam núcleos específicos, podem surgir anos depois da instalação | Hochschild et al. (Google), HotOS'21 ([PDF](https://sigops.org/s/conferences/hotos/2021/papers/hotos21-s01-hochschild.pdf)) | V |
| Centenas de CPUs com SDC em centenas de milhares de máquinas (Meta); exemplo real: **descompressão calculou o tamanho do arquivo errado**; taxa citada em 2022: "one in thousand silicon devices" | Dixit et al., [arXiv:2102.11245](https://arxiv.org/abs/2102.11245); [arXiv:2203.08989](https://arxiv.org/abs/2203.08989) | V |
| Spanner detecta ou previne SDC de hardware "several times per week", e para cada detectado há não detectados | Google, 2022 ([pub](https://research.google/pubs/detection-and-prevention-of-silent-data-corruption-in-an-exabyte-scale-database-system/)) | V (abstract) |
| Gemini: SDC "extremamente raro", mas "esperar eventos de SDC afetando o treino a cada semana ou duas" | [arXiv:2312.11805](https://arxiv.org/abs/2312.11805) | V |
| Llama 3 405B: 54 dias, até 16 mil H100, 466 interrupções (419 inesperadas); **6 por "Silent Data Corruption/GPU"** (só os detectados) | [arXiv:2407.21783](https://arxiv.org/abs/2407.21783) | V |
| Taxa por mil dispositivos para GPU; número exato do estudo Alibaba (TACO 2024, >1 milhão de processadores) | — | **não encontrado / só secundário** |

Derivação (deriv.): 6 eventos / ~16.384 GPUs / 54 dias ≈ 0,04% por GPU nesse período, como limite inferior (só detectados). Nota: a tabela do relatório Llama 3 tem inconsistência interna de percentuais (148/419 = 35,3%, não 30,1%).

**(c) Não-determinismo de ponto flutuante e decodificação aritmética**

| Fato | Fonte | Conf. |
|---|---|---|
| cuBLAS: mesmos bits entre execuções **só** para o mesmo toolkit, mesma arquitetura e mesmo nº de SMs; não é garantido entre versões; garantia some com multi-stream/atomics | [NVIDIA cuBLAS](https://docs.nvidia.com/cuda/cublas/index.html#results-reproducibility) | V |
| cuDNN: "**across different architectures, no cuDNN routines guarantee bitwise reproducibility**" | [NVIDIA cuDNN](https://docs.nvidia.com/deeplearning/cudnn/backend/v9.19.0/developer/misc.html) | V |
| Ordem das operações altera o resultado mesmo com IEEE 754 correto; FMA na GPU vs possivelmente não na CPU | [NVIDIA FP whitepaper](https://docs.nvidia.com/cuda/floating-point/index.html) | V |
| PyTorch: resultados "não são garantidos entre releases, commits ou plataformas; podem não ser reproduzíveis entre CPU e GPU mesmo com mesma seed" | [PyTorch docs](https://docs.pytorch.org/docs/2.9/notes/randomness.html) | V |
| Qwen3-235B, T=0, 1.000 amostras: 80 respostas distintas (divergência no token 103); causa principal = falta de *batch invariance*; com kernels batch-invariantes, 1.000 saídas idênticas. Garante **mesma máquina**, não entre GPUs | Thinking Machines, 10/09/2025 ([blog](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/)) | V |
| **Medição direta em compressão (PMATIC, ICLR 2026):** Llama 3.1, encoder em M2 Pro e decoder em M4 Max → **nenhum arquivo decodificou** com aritmética padrão; discrepância de logit ~0,01–0,02. Compressor e decompressor precisam de predições "exatamente iguais", senão "cascading decoding failures" | Adler & Tang ([arXiv:2601.10678](https://arxiv.org/abs/2601.10678)) | V |
| Hybrid-LLM (2026): codificação em L4 na nuvem, decodificação em RTX 3090: falhou no **token 12**; correção por arredondar logits a 3 casas + softmax em float64 (<0,002 bpc), só um par de hardware testado | [arXiv:2603.25526](https://arxiv.org/abs/2603.25526) | V* |
| DZip: decodificação "deve rodar no mesmo hardware" da codificação; em CPU é reprodutível mas "bem mais lenta" | [arXiv:1911.03572](https://arxiv.org/abs/1911.03572) | V |

*Por que 1 ULP quebra o decodificador (deriv., conceitual + simulação não revisada):* o decodificador mantém um intervalo inteiro e escolhe o símbolo comparando o código com frequências cumulativas inteiras vindas do modelo. Se uma fronteira de CDF difere entre codificador e decodificador, o estado diverge; num modelo autorregressivo/adaptativo o contexto seguinte também diverge, e o erro em cascata é irrecuperável. Simulação do pesquisador (V=256, CDF 2¹⁶, logits fp32 aleatórios): perturbar *todos* os logits em ±1 ULP mudou a CDF quantizada em ~2,95% das posições; perturbar *um* logit, em ~0,025%. A quantização absorve a maior parte, mas em 10⁶ símbolos ainda sobram ~3×10⁴ CDFs diferentes — e basta uma divergência efetiva.

**(d) Soluções conhecidas e seu custo**

| Solução | Fato | Fonte | Conf. |
|---|---|---|---|
| Redes de inteiros | Range coding "pode falhar catastroficamente se o cálculo do prior diferir ligeiramente entre emissor e receptor"; arredondamento é "frequentemente dependente de plataforma" | Ballé, Johnston, Minnen, ICLR 2019 ([Google](https://research.google/pubs/integer-networks-for-data-compression-with-latent-variable-models/)) | V (abstract) |
| Decoder quantizado | Reconstrução bit-exata cross-platform; custo +0,5% BD-rate (pesos/ativações 16 bits) a +7,9% (pesos 8 bits) | Koyuncu et al., ICASSP 2024 ([arXiv:2312.11209](https://arxiv.org/abs/2312.11209)) | V |
| JPEG AI | Entropia **toda em inteiros** por causa de dispositivos que "podem não decodificar corretamente" por diferença de float; padrão ISO/IEC 6048-1:2025 (texto normativo **não lido**) | [arXiv:2503.16288](https://arxiv.org/abs/2503.16288); [ISO](https://www.iso.org/standard/88911.html) | V (paper) / NV (norma) |
| Float controlado em software | NNCP v1: biblioteca C própria com fp32 IEEE básico (FMA, divisão, raiz), "para resultados determinísticos em qualquer CPU e SO". NNCP v2: PyTorch determinístico, garantido **só com hardware e software idênticos** | Bellard ([v1](https://bellard.org/nncp/nncp.pdf), [v2](https://bellard.org/nncp/nncp_v2.1.pdf)) | V |
| ts_zip | Autor afirma resultado independente de GPU/CPU/threads, mas diz que "pode comprimir (e esperançosamente descomprimir)", é experimental e **sem compatibilidade retroativa entre versões**; sem teste cross-hardware na página | [Bellard](https://bellard.org/ts_zip/) | V |
| Tolerar o descasamento | PMATIC quantiza probabilidades e envia *helper bits* para casos de fronteira; custo medido no enwik8: razão sobe de **0,078 para 0,249** com δ=0,01 (3,2x pior) | [arXiv:2601.10678](https://arxiv.org/abs/2601.10678) | V |
| Emular Tensor Cores em CPU | Hawkeye (MLSys 2026) reproduz exatamente em CPU o matmul de Ampere/Hopper/Lovelace modelando arredondamento e ordem de acumulação | [arXiv:2603.20421](https://arxiv.org/abs/2603.20421) | V* |
| Tabelas inteiras (codecs clássicos) | RFC 8878 (zstd): tabelas FSE são contagens inteiras com soma `1 << Accuracy_Log` | [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878) | V |

Não achei trabalho que faça aritmética puramente inteira de ponta a ponta para um LLM grande; só quantização + kernels de ordem fixa (cmix-neif, NNCP/NC) ou controle de ponto flutuante em CPU.

**(e) O modelo como ponto único de falha e deriva de versão**

| Fato | Fonte | Conf. |
|---|---|---|
| zstd: `Dictionary_ID` identifica o dicionário "necessário para decodificar" o frame; sem ele, "o decodificador precisa saber qual usar". Se o dicionário não está disponível, o payload não é decodificável | [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878) | V |
| **Precedente de checksum do modelo no envelope:** zstd com dicionário externo em RFC 9842 leva o **SHA-256 do dicionário** no cabeçalho de 40 bytes | [RFC 9842](https://www.rfc-editor.org/rfc/rfc9842) | V |
| Brotli: dicionário estático faz parte da **especificação** (RFC 7932) — lock-in resolvido por padronização | [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932) | V |
| Perda de modelo público: thread de 03/07/2025 relata `runwayml/stable-diffusion-v1-5` retornando 404 no Hugging Face (causa e data de fonte única, existem espelhos) | [HF forum](https://discuss.huggingface.co/t/license-agreement-error-runwayml-stable-diffusion-v1-5-returns-404/161673) | NV |
| Estudo específico de perda de modelo em compressão neural | — | **não encontrado** |

**(f) Propagação de erro e delimitação nos formatos clássicos**

| Formato | Delimitação | Fonte | Conf. |
|---|---|---|---|
| Codificação aritmética | "Extremamente vulnerável a qualquer erro" (Huffman tende a ressincronizar; aritmética "não tem capacidade de resistir a erros"); em PMATIC: "extremamente sensível numericamente e vulnerável a erros em cascata" | Spiteri & Buttigieg 2010 ([PDF](https://www.scitepress.org/Papers/2010/29444/29444.pdf)); [arXiv:2601.10678](https://arxiv.org/abs/2601.10678) | V |
| zstd | Frames independentes; dentro do frame cada bloco (≤128 KB) depende dos anteriores; `Content_Checksum` opcional = 4 bytes baixos do XXH64, verificado só ao fim (detecta, não localiza) | [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878) | V |
| xz | Check por Block (None/CRC32/CRC64/SHA-256), CRC32 em cabeçalhos e índice; decoder "MUST indicate an error" se o Check falhar | [xz-file-format](https://github.com/tukaani-project/xz/blob/master/doc/xz-file-format.txt) | V |
| bzip2 | Blocos ≤900 kB independentes, delimitador de 48 bits e CRC32 por bloco; `bzip2recover` extrai blocos intactos; chance de erro não detectado "~1 em 4 bilhões por arquivo" | [manual](https://sourceware.org/bzip2/manual/manual.html) | V |

**(g) Mitigações padrão:** *scrubbing* (>60% dos LSE descobertos, ciclo completo em ~2 semanas); ECC de DRAM; checksum end-to-end; erasure coding; 3-2-1 (CISA, [PDF](https://www.cisa.gov/sites/default/files/publications/data_backup_options.pdf), V). Para evidência forense, xxHash/CRC32C só detectam corrupção **acidental**; hash criptográfico (BLAKE3/SHA-256) é o apropriado para integridade probatória (inf.).

### 5.2 Matriz de riscos priorizada (para um produto com reconstrução bit-exata e uso forense)

Probabilidade: **A**lta (já observada em medição direta ou rotina), **M**édia, **B**aixa. Impacto: **C**rítico (perda irrecuperável em massa), **Al**to (perda de objeto/segmento), **Mo**derado.

| # | Risco | Prob. | Impacto | Evidência-chave | Mitigação conhecida | Residual para o Syntra |
|---|---|:-:|:-:|---|---|---|
| R1 | **Decodificador neural em float diverge do codificador** (outro hardware/driver/biblioteca) → decodificação em cascata, segmento irrecuperável | **A** | **C** | PMATIC: 0 arquivos decodificados M2 Pro→M4 Max; Hybrid-LLM falha no token 12; NVIDIA: sem garantia entre arquiteturas | Decoder em inteiros (Ballé 2019; JPEG AI; Koyuncu: +0,5–7,9% BD-rate); CPU com FP32 IEEE controlado (NNCP/NC); PMATIC (custo 3,2x no tamanho); hardware+software pinado | **Exige decisão de arquitetura antes de qualquer treino.** Sem ela, nenhuma garantia de bit-exato. |
| R2 | **Deriva de versão** (toolkit, driver, PyTorch, kernels, versão do próprio modelo) invalida essências antigas | **A** (certeza em anos) | **C** | cuBLAS/PyTorch não garantem entre versões; ts_zip: "sem compatibilidade retroativa" | Congelar decoder de referência (binário+hash), `model_digest` no envelope (precedente RFC 9842), corpus de regressão dourado, política de migração/re-encode | Hoje o envelope referencia dicionário por digest; falta o análogo para modelo/runtime. |
| R3 | **Perda do modelo/dicionário** (ponto único de falha): todo o dado codificado vira irrecuperável | **B** | **C** | RFC 8878: sem dicionário, sem decodificação; `409 dictionary_unavailable` no próprio Syntra | 3-2-1 do modelo, erasure coding, digest no envelope, restauro testado; modelo pequeno (poucos GB) é barato de replicar (inf.) | Já existe em escala pequena (dicionários); com modelo de GB a perda é mais cara e a derivação de versão (R2) agrava. |
| R4 | **Erro de 1 bit propaga até o fim do segmento** num stream aritmético/neural | **M** | **Al** | Aritmética não ressincroniza; zstd/xz/bzip2 limitam por frame/bloco/CRC | Segmentos curtos independentes com contexto reiniciado e hash por segmento (inf.); erasure sobre a essência | Custo em razão de compressão (contexto reiniciado piora o modelo). |
| R5 | **Bit rot / corrupção silenciosa em storage** | **M** | **Al** | 0,86% dos discos nearline com ≥1 mismatch em 41 meses; LSE 8,5% nearline; 33% de P(≥1 erro) em 5 TB lidos a UBER 10⁻¹⁴ | BLAKE3 end-to-end (**já existe**), *scrubbing*, erasure, 3-2-1 | Syntra detecta (422) mas **não repara**: falta camada de redundância. |
| R6 | **SDC em CPU/GPU durante a codificação** gera essência errada que parece válida | **B–M** | **Al** | Google: "a few per several thousand machines"; Meta: 1 em mil; Llama 3: 6 eventos detectados/54 dias | *Verify-on-write* em **dispositivo diferente** (o do Syntra hoje roda no mesmo processo); hash do original | O verify local não prova decodificabilidade em outra máquina (inf.). |
| R7 | **Erro de DRAM** corrompe dado em trânsito | **M** | **Al** | 1,3% das máquinas com erro não corrigível/ano; ZFS menos resiliente a corrupção de memória | ECC obrigatório no hardware on-prem | Requisito de hardware a documentar. |
| R8 | **Binários/executáveis**: 1 bit errado inutiliza o arquivo; modelos neurais pioram aqui | **M** | **Al** | DZip: perde para 7zip em executável (2,15 vs 2,11 bpc, com modelo de ~25% do arquivo) | Compressão clássica + hash; ver Seção 7 | — |
| R9 | **Falha de rede/transferência** | **M** | **Mo** | (inf.) TCP/TLS mais hash end-to-end do objeto cobrem; BLAKE3 já é conferido na leitura | Hash end-to-end (**já existe**) | — |
| R10 | **Dependência de acelerador** (GPU/driver descontinuados) para reconstruir arquivo antigo | **M** | **C** | cuDNN/cuBLAS dependem de arquitetura; emulação em CPU é possível mas lenta (Hawkeye, DZip em CPU) | Decoder de referência em CPU (aceitando throughput menor) como caminho de arquivo | Define se o produto é "arquivo de longo prazo" ou "cache". |

**Conclusão da seção (inf.):** os riscos R1–R3 e R10 são **inexistentes** em codecs clássicos (decoders determinísticos, especificados em RFC, com tabelas inteiras) e **dominantes** em compressão neural. O contrato "reconstrução bit a bit, auditável" do Syntra é, hoje, sustentado precisamente por essa propriedade. Qualquer caminho neural precisa **provar determinismo entre plataformas** antes de ser chamado de lossless.


---

## 6. Análise de custo: quando processar custa mais que armazenar

### 6.1 Modelo

Compara-se a abordagem **neural (N)** com a melhor abordagem **clássica (C)** (zstd/xz + dedup), não com "não comprimir" (ver 6.5). Todas as grandezas são por GB de dado bruto.

**Margem por GB** (valor líquido de trocar C por N):

```
m = ΔS · p · T · r  −  (c_enc,N − c_enc,C)  −  n · c_dec,N

ΔS      = S_N − S_C            ganho fracionário de armazenamento da N sobre a C (adimensional)
p       = preço do armazenamento       [US$/GB·mês]
T       = retenção                     [meses]
r       = fator de redundância física  (1,0 se p já é por GB lógico, como nos preços de nuvem)
c_enc,X = P_X / θ_enc,X                [US$/GB]  com P = US$/hora do recurso e θ em GB/h  (1 MB/s = 3,6 GB/h)
c_dec,N = P_N / θ_dec,N                [US$/GB]
n       = nº de leituras (reconstruções completas) durante a vida do dado
```

**Escala mínima** (pagar o custo fixo):

```
D*  =  (C_treino + C_pesquisa + R · M · p · T · r)  /  m          (só existe se m > 0)

C_treino   = (GPUs × horas × US$/GPU-h)
C_pesquisa = engenharia/pesquisa até produto (PARÂMETRO, não tem fonte)
M, R       = tamanho do modelo [GB] e nº de réplicas mantidas nos nós de decodificação
```

**Condições de existência (equivalentes a m > 0):**

```
vazão mínima:      θ_N  >  P_N / (3,6 · ΔS · p · T · r)        [MB/s], com n = 0
leituras máximas:  n*   =  (ΔS · p · T · r − c_enc,N) / c_dec,N
amortização do modelo:  D  >  M / ΔS                            [GB]  (Seção 3.4)
energia:           E_proc [Wh/GB]  <  ΔS · e_arm · T_anos · PUE · 1000   (e_arm em kWh/GB·ano)
```

### 6.2 Parâmetros (fonte e confiança)

| Parâmetro | Valor | Fonte / tipo |
|---|---|---|
| p (nuvem) | S3 Standard 0,023; R2 0,015; B2 0,00695; Glacier IR 0,004 (US$/GB·mês) | Seção 4.3 — **V** |
| p (on-prem, tudo incluído) | 0,002 US$/GB·mês (HDD ~US$30/TB ÷ 60 meses × 1,5 erasure × ~2,5 para servidor/rede/energia/operação) | **inferência própria** (HDD US$28–41/TB: fonte secundária) |
| T | 60 meses | premissa |
| P_N (GPU nuvem) | L4 US$0,805/h on-demand (g6.xlarge) | Vantage — **V** (agregador) |
| P_N (GPU própria) | US$0,15/h = (US$2.000 ÷ 18.396 h de vida útil a 70%) + 0,3 kW × PUE 1,5 × US$0,10/kWh | **inferência própria** |
| P_C (CPU nuvem / própria) | US$0,0446 / US$0,02 por vCPU·h | Vantage **V** / inferência própria |
| θ_N | 0,25 / 1 / 4 MB/s (pess./real./otim.) | ts_zip ~1 MB/s (RTX 4090), L3TC-200K 4,35 MB/s (A100), FineZip 0,67 KB/s…; Seção 3.4 — **V** |
| θ_C (referência de custo clássico) | **xz -9e ≈ 5 MB/s por núcleo → 18 GB/h**, medido no benchmark (i9-14900HX, mix real, Seção 10). A primeira versão deste relatório usava zstd -22 a 1,4 MB/s (LTCB, hardware antigo) | **medido** (notebook; núcleo de servidor tende a ser mais lento) |
| ΔS | 0,03 / 0,06 / 0,10 | ts_zip vs xz ≈ 0,062; L3TC vs zstd-22 ≈ 0,048; Heurtel-Depeiges vs LZMA2 ≈ 0,07; LLM grande sem contar modelo ≈ 0,11 — **V/deriv.**; **só vale para texto** |
| n | 0,1 (frio) / 1 / 5 (pessimista) | premissa |
| C_treino | US$2 mil (8×A100 × 3 dias a US$2,79) / US$5,8 mil (8×H100 × 1 semana a US$4,29) / US$184 mil (64×H100 × 4 semanas) | Lambda — **V** para os preços; durações são premissa |
| C_pesquisa | US$150 mil / 300 mil / 600 mil | **inferência própria — sem fonte** |
| M | 0,2 / 1 / 16 GB | premissa (cf. ts_zip 0,169 GB; 7B ≈ 14 GB) |

### 6.3 Cenários numéricos (texto, o único tipo com ganho neural demonstrado)

Calculados por [docs/anexos/breakeven.py](anexos/breakeven.py) com os parâmetros de [cenarios_breakeven.json](anexos/cenarios_breakeven.json); a varredura de vazão mínima está em [sensibilidade.py](anexos/sensibilidade.py) e as demais contas derivadas (energia, amortização do modelo, latência, dimensionamento) em [contas_derivadas.py](anexos/contas_derivadas.py). Margem em US$ por GB de dado bruto.

| Cenário | ΔS | θ (MB/s) | n | Ganho de armazenamento | c_enc,N−c_enc,C | n·c_dec | **Margem m** | **Escala mínima D\*** |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| **Nuvem, otimista** (S3 Std, L4) | 0,10 | 4 | 0,1 | 0,138 | 0,053 | 0,006 | **+0,079** | **~1.920 TB (≈1,9 PB)** |
| **Nuvem, realista** | 0,06 | 1 | 1 | 0,083 | 0,221 | 0,224 | **−0,362** | **nunca** |
| **Nuvem, pessimista** | 0,03 | 0,25 | 5 | 0,041 | 0,892 | 4,472 | **−5,323** | **nunca** |
| **On-prem, otimista** (HDD, GPU própria) | 0,10 | 4 | 0,1 | 0,012 | 0,009 | 0,001 | **+0,0017** | **~92.000 TB (≈92 PB)** |
| **On-prem, realista** | 0,06 | 1 | 1 | 0,0072 | 0,041 | 0,042 | **−0,075** | **nunca** |
| **On-prem, pessimista** | 0,03 | 0,25 | 5 | 0,0036 | 0,166 | 0,833 | **−0,995** | **nunca** |

**Vazão mínima por cenário de armazenamento** (L4 US$0,805/h, T = 60 meses, só 1 passagem de codificação; MB/s por GPU):

| Armazenamento | ΔS = 0,02 | ΔS = 0,08 | ΔS = 0,20 | ΔS = 0,40 |
|---|---:|---:|---:|---:|
| S3 Standard | 8,1 | 2,0 | 0,81 | 0,41 |
| R2 Standard | 12,4 | 3,1 | 1,2 | 0,62 |
| B2 | 26,8 | 6,7 | 2,7 | 1,3 |
| S3 Glacier IR | 46,6 | 11,7 | 4,7 | 2,3 |
| On-prem HDD tudo incluído | 93,2 | 23,3 | 9,3 | 4,7 |

Leitura: **só o armazenamento caro de nuvem (S3 Standard, R2) com ΔS alto fecha a conta, e só em escala de PB**. Em armazenamento barato (B2, Glacier, HDD próprio) a vazão mínima exigida (7–93 MB/s por GPU) está 1–2 ordens acima do que existe medido (≤4 MB/s).

**Quente vs frio (n\*):** nuvem otimista n\* ≈ 1,5 leituras; on-prem otimista n\* ≈ 0,15 (só dado essencialmente nunca lido). Qualquer dado lido mais que ~1 vez em 5 anos **perde** com a abordagem neural, mesmo no melhor cenário.

**Latência de reconstrução (deriv.):** a 1 MB/s, um objeto de 100 MB leva ~100 s; de 1 GB, ~17 min; de 10 GB, ~2,8 h (a 4 MB/s: 25 s / 4 min / 42 min). Frente a NVMe (7.000 MB/s) e HDD (285–302 MB/s) a diferença é de 2–4 ordens de grandeza. **Dado quente é inviável** (inf.).

### 6.4 Energia: o balanço também fecha contra

Armazenar 1 TB em HDD consome 1,89–2,83 kWh/ano (Seção 4.5). Com PUE 1,52, 5 anos e ΔS = 0,06, **o que a economia de 6% poupa em energia de armazenamento é 0,86–1,29 Wh/GB**; processar 1 GB a 1 MB/s numa GPU de 72 W (TDP; não é medição) gasta **30 Wh/GB com PUE, por passagem**. Razão (deriv.): **processar custa 24–35x a energia que economiza** (14–71x variando ΔS de 0,10 a 0,03 e a energia de armazenamento entre HDD ocioso e ativo). A 4 MB/s ainda é 6–9x. A conta fica favorável apenas em armazenamento de flash muito caro e energia de armazenamento mais alta (SSD ativo 3,56 kWh/TB·ano) com ΔS alto (inf.).

### 6.5 O que o modelo diz sobre a compressão **clássica** (para não perder a perspectiva)

Contra "não comprimir", a compressão clássica rende muito mais do que custa:

| Contexto | Ganho (60% de economia, T = 60 meses) | Custo de comprimir 1 GB (xz -9e em vCPU, ≈5 MB/s medido) | Retorno |
|---|---:|---:|---:|
| Nuvem (S3 Std) | US$0,828/GB | US$0,0025 | **≈334x** |
| On-prem | US$0,072/GB | US$0,0011 | **≈65x** |

Cada passo adicional de esforço clássico (zstd -1 → -19 → xz -9e) tem retorno marginal decrescente: é a análise marginal que o benchmark da Seção 10 mede (Δ razão ÷ Δ custo de CPU por classe).

### 6.6 Limites do modelo (honestidade)

1. **ΔS é só de texto.** Para binários/estruturados/mídia o ganho neural medido é menor ou desconhecido (Seção 7); os cenários acima são, portanto, **otimistas para dado genérico**.
2. A vazão θ_N é de **ts_zip/L3TC em GPUs de topo**; GPUs baratas (L4) serão mais lentas (inf.). Usei o custo da L4 com a vazão de uma 4090/A100: viés a favor da neural.
3. Não incluí: custo de engenharia de determinismo (Seção 5, que pode somar +3,2x no tamanho com PMATIC ou 4–5x de lentidão em inteiros), banda de rede, falhas, nem o custo de oportunidade da GPU.
4. C_pesquisa e as durações de treino são **premissas sem fonte**; mudam D\* linearmente, mas **não mudam o sinal de m** — e o sinal é o que decide.
5. Preços de NAND/DRAM subiram ~3,6x em 2026 (Seção 4.3). Se o armazenamento de flash virar o padrão em camada quente, p sobe e a vazão mínima exigida cai; é o único eixo que favorece a neural e merece reavaliação semestral.


---

## 7. Comparação por tipo de arquivo

Taxa típica = **razão comprimido ÷ original** ou razão de compressão conforme indicado; números com fonte das Seções 3–4, ou do README do Syntra (corpus próprio de 85 MB, **sintético/não público**, rotulado "Syntra"). "Risco de corrupção" aplica o mapa da Seção 5: **baixo** = decoder clássico com delimitação por bloco/CRC; **alto** = stream neural/aritmético em float (R1, R2, R4).

| Tipo | Algoritmo clássico dominante | Taxa típica | Tolerância a perda | Viabilidade da abordagem neural (evidência) | Risco de corrupção (neural) | Veredito |
|---|---|---|---|---|---|---|
| **Texto** | xz/zstd/brotli/bzip2 | enwik9: xz 19,7%; zstd -22 21,6%; bzip2 25,4% | nenhuma (auditoria) | **Média-alta, a única com sinal.** cmix 10,8%, NNCP 10,66%, ts_zip 13,5% (modelo não contado); com modelo contado e fora de distribuição, 3–8 p.p. sobre zstd/LZMA2. Vazão 0,0001–4 MB/s | alto | **Nicho frio/arquivo**, sob gates do roadmap; **não** no caminho quente |
| **Código-fonte** | zstd/xz (+ dicionário) | **medido** (Silesia/samba, bloco 1 MiB): zstd -3 3,02; zstd -19 3,32; xz -9e 3,37; bzip2 3,19; dicionário em 64 KiB +2,1% | nenhuma | **Hipótese promissora, 1 trabalho:** LLM com *symbol ranking* + bzip3/zstd, até 82% de ganho relativo sobre zstd/bzip2 a ≤0,6 MB/s (8×H100), só compressão medida | alto | **A testar** (Fase 2) |
| **Imagem — lossless** | PNG → WebP → JPEG-XL; JPEG já existente → recompressão Lepton-like | JPEG-XL 2,79 bpsp vs PNG 4,23 (DIV2K); **Lepton: −22,7% em 203 PiB** reais de JPEG (Dropbox, NSDI'17) | nenhuma | **Baixa.** Neural lossless: −0,01 (Kodak) a −0,25 bpsp sobre JPEG-XL, decode ~20x mais lento (1,8 s vs 0,08 s) | alto | Neural: **não**. **Recompressão específica de formato (JPEG, PNG, ZIP): sim** — é a "próxima fronteira" que o próprio README do Syntra aponta |
| **Imagem — lossy** | JPEG/WebP/AVIF | JPEG AI: 12,5–27,9% sobre VVC intra (média de 7 métricas) | só sob **contrato diferente** (nunca auditoria) | Alta em ganho, mas lossy e **a norma exclui lossless/bit-exato** | n/a (lossy) | Outro produto, outro endpoint e outras garantias |
| **Áudio** | FLAC (lossless); Opus/AAC (lossy) | FLAC-8: VCTK 2,32; LibriSpeech 1,74 (razão de compressão) | lossless: nenhuma | **Baixa.** LM +18% (16 bits), **FLAC vence em 24 bits**; são estimativas pela perda, sem bitstream real | alto | **Não** (lossless); lossy neural = outro contrato |
| **Vídeo** | H.264/HEVC/AV1 (já comprimido na origem) | re-comprimir sem perdas ≈ 0% (README Syntra: classificado `media`, gravado `stored`) | lossy tolerado só com reencode | Codec neural: −21% vs VVC, mas **precisa de vídeo bruto** e é lossy; reencode de vídeo já comprimido é transcodificação com perda | n/a | **Não** para arquivo bit-exato; só pipeline de produção de vídeo (fora de escopo) |
| **Binários/executáveis** | xz/zstd (+ filtros de instrução tipo BCJ) | `mozilla` (51 MB): 7zip 2,11 bpc; cmix 1,39 | nenhuma (1 bit errado invalida) | **Baixa.** DZip (neural) **2,15 bpc: perde para o 7zip**; só cmix (CM clássico) ganha, a ~1,6 KB/s | alto | **Não neural**; clássico + hash |
| **Dados científicos (arrays de float/int)** | `byte_split`/`delta` + zstd/xz; FPC/ZFP lossless | Syntra: `telemetria.f32` −54,4% (`byte_split+xz`); `serie.i64` −88,0% | nenhuma | **Média, lenta:** FADE (aprendido online) CESM 2,939 vs 7z 1,829, a ~76/69 KB/s | alto | **Clássico especializado**; neural só se vazão deixar de importar |
| **Bancos de dados / dumps / colunar** | encodings colunares (dict/RLE/delta) + zstd; `bzip2` p/ SQL texto | Syntra: `dump.sql` −91,9% (`bzip2:9`); `vendas.csv` −83,6% (colunar+xz) | nenhuma | **Desconhecida:** nenhum benchmark neural direto contra zstd/xz em logs/SQL/séries foi encontrado | alto | **Clássico**; neural não tem evidência |
| **Já comprimido / criptografado** | nenhum (entropia ≈ 8 bit/byte) | 0,0% (Syntra: `aleatorio.bin` → `stored`) | n/a | **A teoria proíbe** (Seção 2.1): nenhum modelo comprime entropia máxima | n/a | **Não comprimir**; apenas dedup por hash |

**Resumo por ordem de viabilidade neural (inf.):** texto ≫ código (a testar) > floats estruturados ≫ imagem/áudio lossless ≈ binários > banco/logs (sem evidência) > mídia comprimida/criptografada (impossível).

**Observação importante sobre o corpus do Syntra:** 5 dos 6 arquivos do corpus que sustentam "78,8%" são estruturados e favoráveis. No benchmark da Seção 10, com dados públicos (Silesia e Canterbury Large; mix de classes reais, blocos de 1 MiB), a economia **medida** é: `zstd -1` 57,4%; `zstd -3` 61,1%; `zstd -9` 63,7%; `zstd -19` 66,1%; `xz -9e` 68,3%; `bzip2 -9` 68,5%; incompressível e já comprimido 0%. Por classe, de binários (`zstd -3` 46%) a XML (`zstd -3` 87,8%). Ou seja, o "~60%" é coerente com o **mix real** com `zstd -3`, mas varia de 0% a ~90% por classe, e o 78,8% do README é um resultado de corpus favorável, não uma expectativa geral.

---

## 8. Viabilidade e recomendação

### 8.1 Respostas diretas

**O projeto é viável como compressão lossless *genérica* por rede neural?**
**Não**, e por quatro razões independentes, cada uma suficiente:

1. **Vazão (V1).** Os melhores compressores neurais práticos medidos rodam a 0,0001–4 MB/s por GPU (ts_zip ~1 MB/s; L3TC-200K 4,35 MB/s com razão pior; LLMs de 7–13B entre 85 e 280 B/s). O requisito de 5 PB/mês exige 1,93 GB/s médios: **482 a 7.716 GPUs** só para a média (Seção 9.2).
2. **Economia (V2).** O ganho sobre o melhor clássico, com modelo contado e fora de distribuição, é de ~3–8 p.p. (só em texto). Com preços de 2026 a margem é negativa em 4 dos 6 cenários; os 2 positivos (ambos "otimistas") exigem ≥1,9 PB em S3 Standard ou ≥92 PB on-prem, com leitura quase nula (Seção 6). O balanço de energia também perde (24–35x).
3. **Exatidão (V4).** Em float, o decoder diverge entre hardwares: **0 de 150 arquivos decodificaram** entre M2 Pro e M4 Max (PMATIC); falha no token 12 entre L4 e RTX 3090 (Hybrid-LLM); só 47% dos pixels bit-exatos entre H100 e L4 (LatentStore); NVIDIA não garante bit a bit entre arquiteturas. A correção custa +0,5–7,9% de taxa (decoder inteiro/quantizado) ou 4–5x de velocidade, ou 3,2x no tamanho (PMATIC) (Seção 5).
4. **Dado.** Os maiores ganhos dependem de não contar o modelo e de contaminação de treino; para binários, banco e logs **não há evidência**; para dado de entropia máxima a teoria proíbe qualquer ganho (Seções 2, 3.4, 7).

**Em que nichos é viável?** (ordem decrescente de plausibilidade, com a evidência que sustenta)

| # | Nicho | Por que | Condição |
|---|---|---|---|
| 1 | **Texto/código em arquivo frio de escala PB** onde o armazenamento é caro (nuvem, flash) e n ≈ 0 | único com ΔS demonstrado (6–11 p.p.); break-even teórico em ~1,9 PB (nuvem, otimista) | decoder de referência pinado/determinístico; modelo pequeno (≤1 GB); vazão ≥ 4 MB/s; **gates G2–G4** |
| 2 | **Conteúdo gerado pelo mesmo modelo que o reconstrói** (imagens de IA generativa) | LatentStore: −78,7% vs PNG em 2 bi de requisições reais; decode 33–67 ms | só esse tipo; exige rótulo (modelo, versão, precisão) e servir pixels fora da plataforma (47% bit-exato entre GPUs) |
| 3 | **Recompressão específica de formato** (JPEG→Lepton-like, ZIP/XLSX, PNG) | −22,7% em 203 PiB reais (Dropbox); lossless, determinístico (aritmética inteira), sem GPU | trabalho por formato; **não é neural** — é o que o README do Syntra já aponta como "próxima fronteira" |
| 4 | **Neural no *plano de controle*** (escolher transform/codec/nível por bloco) | custo de µs–ms por bloco, **não toca o bit-exato** (o plano fica registrado no envelope) | **Medido (T1.2, Seção 10.9): não vale agora.** `zstd-3` fixo já está a 1,1–1,6% do oráculo; o melhor classificador (CART/logística) fica a 0,4–0,8% mas não cumpre ganho ≥1% sobre o *probe* nem custo ≤2% do zstd-3 (custo medido em numpy; estimado em código nativo: 1,4% a 1 MiB). Reabrir só com medição nativa |
| 5 | Lossy sob **contrato próprio** (imagem/vídeo/áudio com tolerância declarada) | ganhos de 8–28% (JPEG AI, DCVC) | outro endpoint, outra garantia; **nunca** em auditoria/forense (miscompression) |

**Quais premissas precisam mudar?**

| Premissa (Seção 1) | Mudança necessária |
|---|---|
| P2 "rede neural nossa reconstrói o dado" | Rede neural **fora do caminho de dados** (decisão/predição de plano) e, no máximo, como camada **fria opcional e assíncrona** sob gates |
| P3 "~60% em geral" | Metas **por classe** com número e corpus público; incompressível = 0% e é correto dizer isso |
| P6 "amostra central de 8 MB basta" | Decisão **por bloco** (ou amostras estratificadas), medida por classe |
| P7 "escala a 5 TB/5 PB" | Processamento **em fluxo** (sem `Bytes` inteiro em RAM), blocos independentes de 64 KiB–1 MiB, paralelismo medido, índice hierárquico (Seção 9) |
| P8 "dedup de arquivo inteiro basta" | **Parcialmente verdadeira (medido, Seção 10.8):** em árvores de arquivos (kernel Linux) o dedup de arquivo inteiro já captura 66–99% e o CDC reprova; em fluxos únicos o CDC ganha sobre não deduplicar, mas `zstd --long` e **delta entre versões** rendem mais. (Meyer & Bolosky, em desktops da Microsoft, mediram bloco a 32% do original: tipo de dado diferente, não medido aqui) |
| P9 "menos bytes = menos I/O" | Só vale se o gargalo for a banda do meio, não a CPU de descompressão; modelar latência por classe (Seção 6.3) |
| P10 "tokens de IA" via compressão | **Linha de produto separada** (seleção de contexto, cache de prefixo/KV); fora desta pesquisa |
| (nova) "bit-exato = hash conferido" | Também **determinismo provado entre plataformas** e redundância/*scrubbing* fim a fim (Seção 5) |

### 8.2 Roadmap de validação (experimentos mensuráveis e critérios de parada)

Métricas comuns a todas as fases: bits por byte (bpB) e razão; bpp/bpsp e PSNR/SSIM **só** se lossy (contrato à parte); vazão de compressão e descompressão (MB/s por núcleo/GPU e agregada); **Wh/GB** (medidor de potência na tomada ou RAPL); **US$/GB líquido** (modelo da Seção 6); **taxa de reconstrução bit-exata** (deve ser 100%); memória de pico; latência p50/p99 de reconstrução.

Corpora públicos sugeridos (disponibilidade e licença **não verificadas** aqui, conferir antes de usar): Silesia, Canterbury Large, Calgary, enwik8/enwik9 (texto); fontes de projeto open source, ex.: kernel Linux em duas versões (código e dedup entre versões); LogHub (logs); binários de pacotes de distribuição (executáveis); CESM/ERA5 (científicos); dumps TPC-H/TPC-DS e Parquet público (banco/colunar); Kodak/DIV2K/CLIC (imagem), LibriSpeech/VCTK (áudio), UVG (vídeo) — estes três só para o eixo lossy/lossless de mídia.

| Fase | Experimento | Medidas | **Critério de parada ("abandonar se…")** |
|---|---|---|---|
| **F0** (1–2 sem) | Benchmark econômico reprodutível com baselines clássicos (Seção 10) | razão, MB/s/núcleo, escala multi-núcleo, penalidade de bloco, ganho de dicionário, núcleos p/ 5 PB/mês | não houver config clássica que cumpra V1 com ≤64 núcleos no pico (então o requisito exige hardware dedicado/offload) |
| **F1** (2–4 sem) | **CDC (FastCDC) + dedup hierárquico** sobre versões reais (ex.: 2 versões de kernel, imagens de VM, backups) — **executado em 3/11/2026 só com kernel e par sintético: Seção 10.8** | economia extra sobre dedup de arquivo inteiro; MB/s; índice B/bloco | ganho extra **< 5%** nos dados-alvo do cliente, ou índice > RAM prevista sem esparsidade |
| **F2** (4–6 sem) | **Linha de base neural em texto/código frio**: ts_zip, NNCP, L3TC e cmix num hardware pinado **contra** xz -9e e zstd -19 (+dicionário) nos corpora do cliente, **modelo contado** | ΔS, θ_enc, θ_dec, Wh/GB, D\* pela fórmula da Seção 6 com preços reais do cliente | **ΔS < 3 p.p.**; ou **θ < 0,5 MB/s** com armazenamento de nuvem (**< 20 MB/s** on-prem); ou D\* > 50% do volume retido do cliente |
| **F3** (4 sem, em paralelo a F2) | **Determinismo cross-hardware:** codificar numa plataforma, decodificar em ≥4 (CPU Intel/AMD, 2 gerações de GPU, 2 versões de driver/CUDA), ≥10⁹ símbolos | **taxa de reconstrução bit-exata** | **qualquer** falha no caminho float ⇒ abandonar o float; caminho inteiro só se custo ≤10% no tamanho e ≤5x na velocidade |
| **F4** (3 sem) | **Injeção de falhas:** 10⁶ *bit flips*, truncamentos, perda de bloco, perda do dicionário/modelo; restauro de modelo a partir de backup | detecção, contenção (≤1 segmento), reparo por erasure | detecção < 100%, ou contenção > 1 segmento, ou restauro do modelo não comprovado |
| **F5** (6–8 sem) | **Piloto** em 1 vertical, 50–100 TB, on-prem, com medidor de energia | US$/GB líquido real, Wh/GB, p99 de latência | margem líquida ≤ 0 com preços e latências reais, ou p99 acima do SLA |

**Regra de decisão agregada:** só entra em produto o que passar **F0–F4**; F2 e F3 são **eliminatórias** para qualquer componente neural no caminho de dados. Os resultados de F0 e F1 estão na Seção 10 (10.2 e 10.8): F0 cumprido para o caminho clássico; **F1 reprovado para árvores de arquivos** e aprovado só para fluxos únicos, onde janela longa e delta entre versões superam o CDC. F1 ainda **não foi medido** em VM, bancos e backups reais.

### 8.3 Recomendação (inferência própria, com base nas Seções 3–9)

1. **Aposentar a ideia de "NN substitui o codec"** como tese de produto genérica. A matemática (V1, V2) e a medição (V4) a derrubam para dado genérico em escala de PB.
2. **Investir primeiro onde a evidência é forte e o risco é baixo:** dedup de arquivo inteiro com referência/delta entre versões (o CDC só para fluxos únicos, com chunks ≥1 MiB, após validar em dados reais), blocos independentes e processamento em fluxo, dicionários treinados, recompressão por formato, recompressão em camadas assíncronas, *scrubbing* e redundância.
3. **Usar redes neurais só onde o custo é µs, a exatidão não é afetada e o ganho medido justifica.** O teste do classificador de plano por bloco (T1.2) mostrou teto de 1,1–1,6% dos bytes e **não justifica** um modelo aprendido agora; regras simples (probe de incompressibilidade + seleção de transform só para blocos numéricos) capturam o que há de valor. Reabrir com features nativas medidas.
4. **Manter uma opção neural barata e isolada:** camada fria de texto/código, atrás dos gates F2–F4, com decoder de referência pinado. Só vira produto se o piloto provar margem positiva.
5. **Separar o eixo "tokens de IA"** do eixo "armazenamento": hoje compartilham o nome do produto, não o mecanismo.


---

## 9. Requisito de 5 PB/mês: viabilidade matemática

### 9.1 O que o requisito significa (confirmado pelo Osney em 2026-11-04)

Cenário-alvo: **uma empresa cliente tem 5 PB de processamento de dados por mês e armazena "um pouco menos que isso"**; a aplicação deve reduzir o **custo do processamento e do armazenamento**. Traduzo assim (a fração retida é **premissa minha**, a ser trocada pelo número real do cliente):

- **Vazão processada:** 5×10¹⁵ B/mês (30 dias) atravessam o motor: ρ = 1,929 GB/s médios.
- **Retenção:** ~90% do processado (parâmetro `ret`; sensibilidade em 0,80–0,95) vira dado armazenado, ou seja, ~4,5 PB novos por mês (≈54 PB/ano bruto).
- **Dois fluxos no motor:** compressão (entrada/retenção) e descompressão (leitura para processar). O volume lido não foi informado; uso 1–3 leituras por GB processado como sensibilidade.
- **"Custo do processamento"** é ambíguo e depende do pipeline do cliente (o que ele chama de processar, onde a compressão fica, se o consumo aceita dado comprimido). Modelo na Seção 9.7; o que perguntar ao cliente está no plano.

As fórmulas valem para a vazão e para a retenção; a retenção entra só nos termos de armazenamento.

| Pico sobre a média (k) | Vazão | Equivalente |
|---|---:|---|
| ×1 (média, mês de 30 dias) | **1,929 GB/s** | 15,4 Gbit/s |
| ×2 | 3,86 GB/s | 30,9 Gbit/s |
| ×4 (premissa de projeto) | **7,72 GB/s** | 61,7 Gbit/s |
| ×8 | 15,43 GB/s | 123,5 Gbit/s |

Em termos de I/O de data center, a média cabe em **uma fração de 1 SSD NVMe** (7.000 MB/s) e em ~1,5 enlaces de 10 GbE; o pico ×4 cabe em **um porto de 100 GbE**. **O requisito é modesto em largura de banda e pesado em CPU/GPU** — a restrição real é o custo computacional por byte.

### 9.2 Quantos núcleos (ou GPUs) o requisito exige

Fórmula: `N = (ρ · k) / (θ · η)`, com ρ = 1,929 GB/s, k = fator de pico, θ = vazão por unidade (MB/s) e η = eficiência de escala. **Os números clássicos abaixo foram medidos** no benchmark da Seção 10 (i9-14900HX, mix de dados reais, blocos independentes de 1 MiB, compressão); η vem da curva medida do zstd-3: **0,95 a 8 núcleos, 0,64 a 16, 0,52 a 24, 0,42 a 32** (N pico usa 0,42, extrapolado acima de 32; N médio usa o η interpolado). Os números neurais são os publicados (Seção 3.4), com **η = 1 (favorável à neural)**. Custo mensal = N médio × US$ por hora × 730 h.

| Pipeline | Economia (mix real) | θ compressão (MB/s/núcleo) | N médio | N pico ×4 | US$/mês (N médio, nuvem) | Origem de θ |
|---|---:|---:|---:|---:|---:|---|
| **zstd -1** | 57,4% | 596 | **3,3** | **26** | **107** | medido |
| **zstd -3** | 61,1% | 378 | **5,3** | **48** | **171** | medido |
| zlib -6 | 60,7% | 39 | 118 | 472 | 3.845 | medido |
| zstd -9 | 63,7% | 98 | 47 | 187 | 1.523 | medido |
| bzip2 -9 | 68,5% | 22 | 206 | 824 | 6.707 | medido |
| zstd -19 | 66,1% | 7 | 662 | 2.647 | 21.546 | medido |
| xz -9e | 68,3% | 5 | 862 | 3.448 | 28.064 | medido |
| **Neural otimista** (L3TC-200K) | +3–8 p.p. só em texto | 4 | **482** | 1.929 | **283.396** | publicado |
| **Neural realista** (ts_zip) | idem | 1 | **1.929** | 7.716 | **1.133.584** | publicado |
| Neural pessimista | idem | 0,25 | 7.716 | 30.864 | 4.534.336 | publicado |
| cmix | idem | 0,0016 | 1.205.633 | 4.822.531 | — | LTCB |

Preços: c7i vCPU US$0,0446/h e L4 US$0,805/h (Vantage, V). **A versão anterior desta tabela usava θ do LTCB (xz-9e a 0,17 MB/s, zstd -22 a 1,4 MB/s), de hardware antigo; as medições mostram xz-9e e zstd-19 ~20–30x mais rápidos** (5–7 MB/s por núcleo), o que muda os números clássicos de milhares para centenas de núcleos, **sem alterar a conclusão sobre a neural**, que continua 2 ordens de grandeza acima.

**Teorema de dimensionamento (deriv.):** para caber em **um servidor de 64 núcleos no pico ×4** é preciso `θ_núcleo ≥ 7.716 MB/s ÷ (64 × η) = 287 MB/s` com η = 0,42 medido. **Só zstd -1 (596) e zstd -3 (378) cumprem** em todas as classes reais; zstd -9 (98) já exige 187 núcleos de pico. Com fator servidor/notebook de 0,6 (núcleo de nuvem mais lento que um P-core de 5,8 GHz; **premissa do benchmark, não medida**), o zstd -3 passa a 81 núcleos de pico e só o zstd -1 (51) segue cabendo. **Atualização (Seção 10.8):** em núcleos homogêneos a eficiência medida foi 0,72–0,91 até 16 núcleos; com η = 0,85 (inferência) o N de pico seria ~15 (zstd -1) e ~24 (zstd -3), contra 26 e 48 com η = 0,42 do híbrido. Acima de 16 núcleos não há dado.

**Camada assíncrona não precisa ser dimensionada no pico.** A recompressão de dado já ingerido pode ser espalhada pelo mês: zstd-19 custa 662 núcleos médios (US$21,5 mil/mês) e xz-9e 862 (US$28 mil/mês) para repassar 5 PB/mês. O ganho de armazenamento de zstd-3 → xz-9e no mix real é de 7,2 p.p.: US$497 mil por coorte em S3 Standard (60 meses) contra US$28 mil de CPU (**~18x**) (deriv.).

### 9.3 Quanto vale (e custa) cada coorte mensal de 5 PB

Retenção de 60 meses no S3 Standard (US$0,023/GB·mês), 5×10⁶ GB por coorte (deriv.):

| Cenário | Economia de armazenamento por coorte | Custo de computar a coorte | Saldo |
|---|---:|---:|---:|
| **Clássico** (57–68% de economia no mix real) | **US$4,0–4,7 milhões** | US$107 (zstd -1) a US$28 mil (xz -9e), medido | **fortemente positivo** |
| Neural, **+6 p.p.** sobre o clássico | US$414 mil | **US$1,13 milhão** (1 MB/s, L4) | **−US$720 mil por coorte** |
| Neural, +10 p.p. | US$690 mil | US$283 mil (4 MB/s) a US$1,13 mi | −US$443 mil a +US$407 mil |

Mesmo no melhor caso o saldo neural é uma fração do valor clássico, e é positivo só com vazão ≥ 4 MB/s **e** armazenamento de nuvem caro. Em tráfego de rede (egress S3 US$0,09/GB): 5 PB/mês = US$450 mil/mês; 60% de compressão clássica poupa US$270 mil/mês, +6 p.p. neural poupa ~US$27 mil/mês contra US$283 mil–1,13 milhão de computação (deriv.).

### 9.4 O gargalo escondido: índice de deduplicação

Dedup global por bloco exige um índice de fingerprints. Com **32 B por entrada** (hash + localização; premissa) e 5×10¹⁵ B/mês:

| Tamanho médio do bloco | Blocos/mês | Índice por mês de dado |
|---:|---:|---:|
| 4 KiB | 1,22×10¹² | **39,1 TB** |
| 8 KiB | 6,10×10¹¹ | **19,5 TB** |
| 64 KiB | 7,63×10¹⁰ | 2,44 TB |
| 128 KiB | 3,81×10¹⁰ | 1,22 TB |
| 1 MiB | 4,77×10⁹ | 0,15 TB |

Comparação: o ZFS exige ≥1,25 GiB de RAM por TiB com dedup (Seção 4.1); a 5 PB/mês isso seria **~6 TB de RAM por mês de dados**. A solução publicada é o **DDFS** (Zhu et al., FAST'08): *Bloom filter* (Summary Vector) + SISL + cache por localidade, que eliminam ~99% dos acessos ao índice em disco e sustentam 100–210 MB/s por *shelf* (V, Seção 4.1). Dedup por amostragem/índice esparso (Lillibridge et al., FAST'09 — **[NV, de memória]**) e domínios de dedup por tenant/volume são as saídas conhecidas. **Conclusão (inf.):** a 5 PB/mês o dedup precisa ser **hierárquico** (arquivo inteiro → blocos grandes → blocos pequenos só para classes que pagam), não "global fino".

### 9.5 Confiabilidade em escala

5×10¹⁵ B = **4×10¹⁶ bits/mês**. Com UBER do datasheet em 10⁻¹⁵ (Seagate Exos; **limite superior**, erros assumidos independentes — premissa que o FAST'08 contradiz), uma leitura completa do volume mensal tem **~40 erros não recuperáveis esperados** (deriv.). A 10⁻¹⁴ (WD Red Plus): 400. **Checksum de ponta a ponta, *scrubbing* e erasure coding são obrigatórios**, não opcionais — o Syntra já tem o primeiro (BLAKE3), e **não tem** os outros dois (Seção 5, R5). Detalhe: o *verify-on-write* local não cobre SDC de CPU/GPU (R6).

### 9.6 Critério de viabilidade matemática (resumo executável)

Um pipeline P é **viável** para o requisito se, e somente se, cumprir **simultaneamente**:

```
(V1)  vazão:      N_pico = 7,72 GB/s / (θ_c · η)  ≤  N_disponível             (≈ θ_c ≥ 287 MB/s/núcleo para 64 núcleos com η = 0,42 medido no híbrido; ≈ 142 MB/s com η = 0,85, homogêneo, inferência)
(V2)  economia:   m = S · p · T · r  −  P/θ_c  −  n · P/θ_d   >  0            (por GB, vs. NÃO comprimir; vs. clássico use ΔS)
(V3)  índice:     (B_mês / bloco) · e  ≤  RAM_índice_disponível                (ou índice hierárquico/esparso)
(V4)  exatidão:   decodificação bit-exata e determinística em ≥ 3 plataformas, taxa = 100%   (Seção 5, R1–R2)
(V5)  integridade: perda esperada ≤ meta, com checksum e redundância fim a fim            (Seção 9.5)
(V6)  latência:   t_leitura = tamanho / θ_d  ≤  SLA da classe de dado                     (Seção 6.3)
```

**Resultado:** compressão clássica em linha de vazão + *dedup* hierárquico + recompressão em camadas satisfaz V1–V6. Compressão neural genérica **falha V1, V2 (no cenário realista), V4 e V6**; só V3 e V5 são independentes do tipo de modelo.


### 9.7 Modelo do cliente: processamento + armazenamento

Premissas (**todas minhas, a trocar por dados do cliente**): zstd-3 no mix real do benchmark (economia s = 61,1%; compressão 378 MB/s e descompressão 1.611 MB/s por núcleo), eficiência de escala 1,0 (otimista), vCPU de nuvem US$0,0446/h (Vantage) ou on-prem US$0,0135/núcleo·h (inferência), retenção de 60 meses, `ret` = fração retida do processado. Cálculo em [docs/anexos/modelo_cliente.py](anexos/modelo_cliente.py).

**Armazenamento** (economia em regime estável = `s × ret × 5 PB × 60 meses × preço`):

| `ret` | S3 Standard (US$0,023) | B2 (US$0,00695) | on-prem (US$0,002, inferido) |
|---:|---:|---:|---:|
| 0,80 | US$3,37 mi/mês | US$1,02 mi/mês | US$293 mil/mês |
| **0,90** | **US$3,79 mi/mês** | US$1,15 mi/mês | US$330 mil/mês |
| 0,95 | US$4,01 mi/mês | US$1,21 mi/mês | US$348 mil/mês |

**Movimento de dados** (economia = `s × 5 PB × custo por GB movido × passagens`; o custo de movimento do cliente é **desconhecido**, os valores abaixo são sensibilidade; a referência verificada é o egress do S3, US$0,09/GB nos primeiros 10 TB e US$0,05/GB acima de 150 TB; tráfego dentro da mesma região pode custar 0):

| Custo por GB movido | 1 passagem | 3 passagens |
|---:|---:|---:|
| US$0 (mesma região) | US$0 | US$0 |
| US$0,01 | US$30,6 mil/mês | US$91,7 mil/mês |
| US$0,05 | US$152,8 mil/mês | US$458 mil/mês |
| US$0,09 | US$275 mil/mês | US$825 mil/mês |

**CPU da compressão e da descompressão** (nuvem, η = 1,0): US$202/mês com 1 leitura por GB processado, US$279 com 3, US$548 com 10 (compressão de 5 PB = US$164/mês).

**Razão entre economia de armazenamento (`ret` = 0,90, 3 leituras) e CPU:** S3 Standard **~13.600x**; B2 ~4.100x; on-prem ~3.900x.

**Leitura (inferência minha):**

1. No cenário-alvo, **a economia de armazenamento domina**; o custo de CPU da solução clássica é de 0,01–0,3% dela. A decisão que importa é a **razão de compressão** que o motor entrega por classe de dado, e não o custo de computar.
2. **Processamento:** o ganho depende de o consumo aceitar dado comprimido e de o gargalo ser movimento de bytes. Se o pipeline do cliente precisar de dado **descomprimido** em cada passagem, a descompressão entra como CPU (barata, ~US$38 por passagem de 5 PB nesta conta) e o ganho de movimento só existe se o transporte entre compressão e consumo for comprimido. **Isso exige conhecer o pipeline do cliente.**
3. O que **não** está no modelo: custo de CPU do cliente que escala com bytes (varreduras, shuffles), latência adicional, formatos columnares que já comprimem (Parquet/ORC) — nesse caso o ganho extra sobre o dado do cliente é bem menor — e custo de migração.

---

## 10. Benchmark e direção de desenvolvimento

### 10.1 O benchmark

Código em [bench/](../bench/) (Python 3, só biblioteca padrão e CLIs `zstd`/`xz`/`bzip2`; sem Rust, que não está instalado nesta máquina). Metodologia, definição de cada métrica e limitações em [bench/README.md](../bench/README.md); tabelas completas em [bench/resultados/resumo.md](../bench/resultados/resumo.md).

```
python3 -I bench/preparar_dados.py --destino DADOS --zips ZIPS \
 && python3 -I bench/syntrabench.py --corpus-dir DADOS --saida bench/resultados/medicoes.json --repeticoes 3 --cpu 0,2,4,6 \
 && python3 -I bench/economia.py
```

| Item | Valor |
|---|---|
| Dados | Silesia e Canterbury Large (públicos, baixados; sha256 de cada arquivo registrado em `medicoes.json`), mais 3 classes **sintéticas** rotuladas (incompressível, logs, já comprimido). Fatia máxima de 8 MiB por arquivo |
| Classes | texto, código, binário, científico, banco de dados, XML, DNA, logs (sint.), incompressível (sint.), já comprimido (sint.) e um **mix real** (as classes reais juntas) |
| Configurações | zstd 1/3/9/19/22 (+`--long=27`), xz 1/6/9e, bzip2-9, zlib 1/6/9, stored; modos arquivo inteiro, **bloco de 1 MiB** e **bloco de 64 KiB**; dicionário zstd treinado em 64 KiB |
| Protocolo | dados em tmpfs, mediana de 3 repetições, núcleos fixados com `taskset`, sha256 do original comparado ao descomprimido em **toda** medição |
| Máquina | i9-14900HX (8 P-cores + 16 E-cores, 32 threads), governor `powersave`, zstd 1.5.7, xz 5.8.3, Python 3.14.4 |
| O que o diferencia de lzbench/Squash | camada econômica ([bench/economia.py](../bench/economia.py), [bench/precos.json](../bench/precos.json)): converte razão e MB/s em núcleos para 5 PB/mês, US$/GB líquido por camada (quente/morno/frio) e cenário de preço, análise marginal e regra de recompressão. Cada parâmetro de preço tem `fonte` e `tipo` (`verificado` ou `inferencia_propria`) |

**Verificações que fiz** (além do relatório do subagente que construiu): 0 violações de bit-exatidão e 0 células com falha; nenhum comentário em nenhum `.py` (tokenização); o teste de validação à mão do modelo bate (zstd -1 a 510 MB/s ⇒ 3,78 núcleos médios, US$123,15/mês, igual ao programa); vazão de 1 P-core em zstd-3 (~500 MB/s) coerente com a referência pública de zstd. Uma primeira execução foi descartada pelo próprio agente por 58 células com erro de configuração; só a segunda execução (553 s) está em `resultados/`. **Não reproduzi a execução inteira.**

### 10.2 Resultados medidos

**Razão (original ÷ comprimido) e vazão de compressão em MB/s por núcleo, blocos de 1 MiB**

| Classe | zstd -3 | zstd -19 | xz -9e | bzip2 -9 |
|---|---:|---:|---:|---:|
| texto | 3,18 (325) | 3,93 (6) | 3,99 (5) | **4,42** (22) |
| código | 3,02 (517) | 3,32 (8) | **3,37** (6) | 3,19 (23) |
| binário | 1,85 (411) | 2,04 (10) | **2,13** (7) | 1,90 (23) |
| científico | 2,11 (348) | 2,40 (8) | 2,74 (5) | **2,82** (22) |
| banco de dados | 2,85 (453) | 3,05 (8) | 3,34 (6) | **3,49** (24) |
| XML | 8,19 (805) | 11,02 (5) | 11,52 (6) | **12,18** (19) |
| DNA | 3,31 (375) | **4,00** (4) | 3,80 (4) | 3,70 (22) |
| logs (sint.) | 4,05 (446) | 5,37 (6) | 5,42 (5) | **6,13** (24) |
| incompressível / já comprimido (sint.) | 1,00 | 1,00 | 1,00 | 1,00 |
| **mix real** | **2,57** (378) | 2,95 (7) | 3,16 (5) | 3,17 (22) |

Descompressão por núcleo: zstd 1,2–3,0 GB/s em todos os níveis (zstd -19 no mix: 1.581 MB/s); xz -9e 96–442 MB/s; bzip2 40–92 MB/s. **Compressão de nível alto custa 50–100x mais CPU que zstd -3; descompressão não.**

**Escala multi-núcleo** (processos independentes, dados distintos; eficiência = agregado ÷ soma das vazões isoladas de 1 P-core)

| N | zstd -3 compressão (MB/s agregados) | eficiência | zstd -3 descompressão | eficiência | zstd -1 compressão | eficiência |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 500 | 1,08 | 2.049 | 1,06 | 711 | 1,03 |
| 8 (só P-cores) | 3.170 | 0,95 | 13.168 | 0,91 | 4.466 | 0,91 |
| 16 | 4.363 | 0,64 | 19.331 | 0,67 | 6.795 | 0,67 |
| 24 | 5.351 | 0,52 | 19.082 | 0,44 | 8.727 | 0,57 |
| 32 | **5.846** | **0,42** | 14.275 | 0,25 | **10.085** | 0,47 |

O agregado a 32 threads **não alcança o pico de 7,72 GB/s com zstd -3** (5,85 GB/s) e alcança com zstd -1 (10,1 GB/s). A queda vem de E-cores (0,54–0,76x de um P-core), SMT e redução de clock sob carga; num servidor homogêneo a curva tende a ser melhor (**confirmado em parte na Seção 10.8**: 0,72–0,91 até 16 núcleos homogêneos; acima disso não há dado). A medição principal usou 4 trabalhadores concorrentes, o que subestima a compressão em 6–11% (blocos de 1 MiB) e até 23% (arquivo inteiro, configs lentas) contra um P-core isolado.

**Penalidade de bloco** (aumento do tamanho comprimido vs arquivo inteiro, mix real)

| Config | bloco 1 MiB | bloco 64 KiB |
|---|---:|---:|
| zstd -1 | 0,3% | 4,5% |
| zstd -3 | 1,3% | 10,3% |
| zstd -9 | 3,5% | 14,5% |
| zstd -19 / xz -9e | ~5% | ~18% |
| bzip2 -9 | 0,9% | 14,2% |

Por classe em 64 KiB com nível alto: texto +34%, código +32%, banco +30–34%, **XML +47–49%**. A 1 MiB o pior caso é código (+12% em zstd-19). **Bloco de 1 MiB é o ponto de equilíbrio entre acesso aleatório e razão; 64 KiB só se paga com dicionário ou exigência de acesso fino.**

**Dicionário treinado** (zstd, blocos de 64 KiB, ~110 KB, treino disjunto do teste; ganho de razão)

| Classe | zstd -3 | zstd -9 | custo de vazão |
|---|---:|---:|---|
| texto | **+7,7%** | **+10,5%** | compressão −14 a −43%; descompressão até −38% |
| código | +2,1% | +4,0% | idem |
| XML | −0,4% | +0,7% | — |
| logs (sint.) | −3,7% | +1,2% | — |

O dicionário foi treinado e testado na **mesma fonte** (provável otimismo) e o treino/armazenamento/versionamento não entra no valor líquido.

**Análise marginal** (nuvem + S3 Standard, camada morna; ganho de armazenamento ÷ custo de CPU de cada passo, texto): zstd-1→3: **3.654x**; zstd-3→9: **105x**; zstd-9→19: **7,9x**; zstd-19→xz-9e: **2,5x**. Na quase totalidade das classes todos os passos "valem". Exceções: DNA zstd-19→xz-9e (**−7,2x**, xz piora) e logs sintéticos zstd-1→3 (−2.639x). Em incompressível e já comprimido nenhum passo vale. **O custo de CPU é de 0,1% a 6% do valor de armazenamento economizado**, então, a preço de nuvem, o ótimo irrestrito é quase sempre a configuração de maior razão.

**Regra de recompressão em camadas** (reempacotar de zstd-3 para zstd-19; M\* = meses de vida restante mínimos para se pagar; **mix real, relatado pelo subagente**; conferi texto, código, binário, científico e banco nas tabelas por classe)

| Armazenamento | M\* |
|---|---:|
| S3 Standard | **3,7 meses** |
| B2 | 12,3 meses |
| on-prem (preço inferido) | 13,0 meses |
| Glacier IR | 21,5 meses |

zstd-19 descomprime tão rápido quanto zstd-3, então a regra vale em qualquer camada de leitura. Para xz-9e: M\* = 3,4 meses (S3, morno) e 11,7 (B2, morno); em camada **quente** (n = 100) com armazenamento barato o reempacotamento para xz-9e **nunca** se paga (o xz descomprime 10x mais devagar).

**Memória (pico de RSS, entrada de 32 MiB):** zstd -3: 44 MB comprimindo / 7 MB descomprimindo; zstd -19: 131 / 14; zstd -22: 442 / 38; xz -9e: 363 / 35.

**Cumprimento de 5 PB/mês** (mix real, bloco 1 MiB, η medido): só **zstd -1 (26 núcleos no pico, US$107/mês) e zstd -3 (48 núcleos, US$171/mês)** cabem em 64 núcleos; zstd -9 pede 187, zstd -19 2.647, xz -9e 3.448 (Seção 9.2). Perda por usar a configuração restrita em vez da ótima irrestrita: US$0,03 a 0,16 por GB (nuvem) e US$0,001 a 0,014 (on-prem).

### 10.3 O que o benchmark diz

1. **O requisito de 5 PB/mês é viável com compressão clássica e é barato em CPU** (US$107–171/mês de núcleos de nuvem para o caminho síncrono), desde que o pipeline seja paralelo e em blocos. A rede neural genérica não passa de 2 ordens de grandeza de distância (Seção 9).
2. **O limite é vazão no pico, não custo.** A preço de nuvem a CPU é irrisória frente ao valor guardado; o que impede usar zstd-19/xz-9e na ingestão é caber nos núcleos disponíveis. Isso aponta para **duas camadas**: ingestão rápida (zstd-1/3) e recompressão assíncrona guiada por M\*.
3. **A eficiência de escala é a incerteza dominante** do dimensionamento: 0,42 a 32 threads no híbrido P+E com SMT, mas 0,72–0,91 até 16 núcleos homogêneos (Seção 10.8). Precisa ser remedida em hardware de servidor acima de 16 núcleos antes de qualquer promessa de capacidade.
4. **Classe importa mais que algoritmo:** a ordem dos vencedores muda por classe (bzip2 vence texto, científico, XML, banco e logs em razão; xz vence código e binário; zstd-19 vence DNA) e incompressível/já comprimido deve ser detectado e gravado como `stored` (zstd-3 detecta a 5,5 GB/s). Isso valida o princípio **"medir em vez de assumir"** do planner atual do Syntra e sugere trocá-lo, no caminho quente, por um classificador barato.
5. **Bit-exatidão:** 0 violações em todas as medições, como esperado de decoders clássicos determinísticos (Seção 5).

**Limites (do benchmark):** CPU de notebook híbrido, não de servidor (fator servidor/notebook fixado em 1,0, provavelmente otimista); fatia de 8 MiB por arquivo (razões não são as do arquivo inteiro); modos em bloco com menos configurações (sem xz-1/6, zlib-1/9, `--long`, nem zstd-22 em 64 KiB); dicionário só em zstd-3/9; escala só com zstd-3/1 em rodadas de 1,5 s e extrapolada acima de 32; P-core vs E-core só em dois arquivos; **energia não medida** (RAPL sem permissão); logs sintéticos não representam produção; **dedup/CDC e compressão neural não foram medidos**; o modelo econômico não tem desconto temporal nem custo de recuperação do Glacier; preços on-prem são inferência própria. O harness em Python mede a compressão 5–13% mais rápida e a descompressão em bloco 17–24% mais lenta que `zstd -b` (causas não confirmadas): superestima o custo de leitura.

### 10.4 Direção de desenvolvimento: o que construir

Princípio: **cada componente precisa ser sustentado por uma condição de viabilidade (V1–V6, Seção 9.6) e por uma medição**. Nada abaixo é um algoritmo de compressão novo; o diferencial é a composição.

| # | Componente | O que faz | Sustentação | Gate / medição pendente |
|---|---|---|---|---|
| A | **Caminho síncrono em linha de vazão** | blocos independentes de 1 MiB (64 KiB sob demanda), zstd-1/-3 por classe, *probe* de incompressibilidade → `stored`; paralelismo por bloco; processamento em fluxo (sem `Bytes` inteiro em RAM) | V1: 26–48 núcleos de pico (medido); bit-exato por construção | rodar em servidor-alvo, medir η real e I/O de disco/rede |
| B | **Planejador por bloco, simples** | *probe* de incompressibilidade (→ `stored`) + `zstd-3` (ou `zstd-1` se a vazão exigir) + **seleção de transform (delta/byte_split) por amostra apenas em blocos numéricos** detectados pela entropia dos deltas; o plano fica no envelope, então o decoder continua determinístico. **Sem modelo aprendido por ora** | **medido (T1.2, 10.9):** `zstd-3` fixo está a 1,1–1,6% do oráculo; o ganho está em `científico` (11–13%) e `dna` (3,4–3,6% em 64–256 KiB); um classificador aprendido não passou o critério de parada | **T1.2 executado**; reabrir só se features nativas medirem ≤2% do zstd-3 e o IC do ganho ficar >1% |
| C | **Recompressão assíncrona guiada por M\*** | repassa blocos já ingeridos para zstd-19/xz-9e/bzip2-9 quando `meses_restantes ≥ M*(classe, preço, camada)`, priorizando por ganho/custo marginal; usa N médio espalhado pelo mês | M\* = 3,7 meses (S3) a 21,5 (Glacier IR); ~18x de retorno sobre o CPU em S3 (Seção 9.2) | medir em carga real e com leitura concorrente |
| D | **Dedup e referência entre versões** | arquivo inteiro (já existe) + **delta/janela longa** contra a versão anterior (`zstd --patch-from` / `--long`) para famílias de arquivos e fluxos de backup; **CDC só para fluxos únicos sem estrutura de arquivo, com chunks ≥1 MiB**; índice Bloom + localidade só se o CDC entrar (Zhu et al., 2008) | **medido (10.8):** CDC reprova em árvores; delta por caminho é 29% menor que arquivo inteiro + zstd-3 no Par B; `--long=31` sem índice supera o melhor CDC; índice de CDC 8–64 KiB não cabe em 1 TB de RAM com retenção >1 mês | **F1 executado**; falta VM/banco/backups reais e a memória de `--long=31` |
| E | **Integridade fim a fim** | BLAKE3 (já existe) + *scrubbing* + erasure coding + *verify* em **nó diferente** + `digest` do dicionário/modelo no envelope (precedente RFC 9842) | V5; R5, R6, R3 da Seção 5; UBER de 10⁻¹⁵ ⇒ ~40 erros por leitura do volume mensal | **F4**: injeção de falhas |
| F | **Planejamento econômico exposto na API** | `/analyze` devolve valor líquido por GB com os preços e a retenção do **tenant**, e a camada recomendada; combina com o ledger e a cobrança por economia (`savings_share`) que o Syntra já tem | é o que o benchmark calcula; transforma "razão" em decisão de negócio | validar com 1 cliente-piloto |
| G | **Camada fria neural (opcional, isolada)** | texto/código sem leitura, decoder de referência pinado | Seção 8, nicho 1; só com os gates F2–F4 | **F2/F3 eliminatórios** |

### 10.5 Diferencial: o que a varredura de concorrentes mostrou (2026-11-04)

Varredura feita por um subagente com busca na web (o relatório completo foi lido por mim; **não reabri cada fonte**). Rótulos: [V] = página ou PDF lido pelo pesquisador; [NV] = só trecho de busca ou fonte secundária. Patentes são citadas como fato bibliográfico, **não como parecer jurídico**.

**Resultado geral:** não foi encontrado produto que faça as quatro peças juntas, mas **três das quatro existem separadas, com fonte, e uma delas está patenteada**.

| Peça da hipótese | Quem já faz (fonte) | Veredito |
|---|---|---|
| **(1) Decisão econômica em US$** | **SCOPe** (Adobe Research, ICDE'23): ILP que escolhe tier e codec por partição minimizando custo de armazenamento, leitura e computação, com preços de tiers do Azure e preditor MLP; alega 50–83% de economia de custo em data lakes de TB a PB ([arXiv:2305.14818](https://arxiv.org/abs/2305.14818), V). **Patente US11990923** (Elasticsearch B.V., prioridade 2020-03-04, concedida 2024-05-21, ativa): modelo de custo financeiro (CPU, armazenamento, rede) escolhe algoritmo e parâmetros; a reivindicação 1 foi lida resumida ([Google Patents](https://patents.google.com/patent/US11990923), V). Arrays vendem só **razão garantida** (Dell 6:1 no PowerStore Gen3, NetApp 4:1 em SAN, Hitachi 4:1) | **Cai como ineditismo conceitual.** Sobra execução. Há **risco de patente** |
| **(2) Plano por bloco aprendido, bit-exato** | **Meta OpenZL** (BSD, anunciado em 2025-10-06): grafo de codecs reversíveis, *trainer* offline, "ML Selector" que escolhe o grafo por entrada, grafo resolvido gravado no frame, **decoder universal sem decisão em runtime**, kernels determinísticos, checksums opcionais por chunk. Em Silesia/`sao`: zstd-3 razão 1,31 a 220 MB/s; xz-9 1,64; **OpenZL 2,06 a 340 MB/s de compressão e 1.200 MB/s de descompressão**; no Meta: Scribe ~15% melhor que zstd, checkpoints PyTorch −17%, embeddings −30%; em texto (enwik) cai para zstd ([arXiv:2510.03203](https://arxiv.org/abs/2510.03203), [engineering.fb.com](https://engineering.fb.com/2025/10/06/developer-tools/openzl-open-source-format-aware-compression-framework), [GitHub](https://github.com/facebook/openzl), V). Escolha por amostragem: **BtrBlocks** (TUM, SIGMOD'23), Vortex, DuckDB ([BtrBlocks](https://www.cs.cit.tum.de/fileadmin/w00cfj/dis/papers/btrblocks.pdf), V). Seleção neural: CodecDB (SIGMOD'21, ~90% de acerto, NV). Comercial: **Granica Crunch** ("ML-powered adaptive compression" em Parquet/Iceberg/Delta, 20–50% além do ZSTD, on-prem air-gapped; [docs](https://docs.granica.ai/crunch/how-it-works), V) | **Cai como diferencial técnico.** Construir isso do zero não é diferencial |
| **(3) Integridade e determinismo verificáveis** | OpenZL (versão do formato e checksums por chunk), Vortex (semente fixa), ZFS/Ceph/restic (hash por bloco; ZFS com dedup `verify` byte a byte), Lepton (round-trip) | **Existe, é requisito mínimo.** Não foi achado "digest de dicionário/modelo no formato" nem "verificação em outro nó" como recurso empacotado |
| **(4) Cobrança por % da economia** | FinOps de nuvem: ProsperOps ("Savings Share"), nOps, Usage.ai, Zesty (V/NV). Compressão de lakehouse: Granica (espelho antigo do site diz "sem economia, sem fatura"; página atual não descreve preço por resultado; NV) | **Cai como ineditismo.** Sobra o escopo: blobs e arquivos genéricos, on-prem, API |

**Outros achados relevantes:** Weka e VAST usam similaridade/delta (VAST liga e desliga a similaridade pelo benefício de redução, não pelo US$); Intel QAT chega a 73 Gb/s no Silesia (Red Hat, NV) e BlueField/ScaleFlux oferecem offload, o que torna a compressão genérica uma commodity em hardware; S3 Intelligent-Tiering, GCS Autoclass e Azure lifecycle **não citam compressão** na documentação (ausência na doc, não prova de inexistência).

**Efeito sobre o posicionamento (inferência minha, apoiada na varredura):**

1. **A hipótese de diferenciação por composição cai de "forte" para "parcial".** O que resta é execução e escopo: motor genérico (blobs e arquivos), **on-premise com API**, planejamento por **valor líquido do cliente** (processamento + armazenamento, Seção 9.7), integridade verificável e cobrança por economia entregue, para **um cliente de escala PB**.
2. **OpenZL passa a ser o baseline obrigatório e candidato a componente.** Em dados estruturados ele reporta ganhos grandes sobre zstd/xz com decoder universal e licença BSD. Se confirmado nos dados do cliente, faz mais sentido **adotá-lo como codec do motor** e diferenciar na camada de decisão do que reconstruir trainer e seletor (tarefa T1.9 do plano). Em texto genérico ele reverte para zstd, então a camada de recompressão por classe continua útil.
3. **Risco de patente (US11990923):** a seleção de compressão por modelo de custo financeiro pode sobrepor a hipótese (1). **Precisa de análise de liberdade de operação por advogado antes de comercializar** (decisão D8 do plano). Só foi lida a reivindicação 1 resumida; não verifiquei se a patente virou produto.
4. **Savings-share exige definir a linha de base da economia** (contra o quê se mede). Compradores de armazenamento estão acostumados com **razão garantida com remédio em hardware**; uma cobrança por % da economia precisa de um critério auditável de medição.

**Não foi possível verificar:** preço atual da Granica (404 na FAQ); termos primários de NetApp, Hitachi e IBM (403 ou PDF ilegível); Veeam, Rubrik, HPE e Qumulo; Cloudflare e Snowflake (não pesquisados); release OpenZL 0.2 (só fonte secundária); vazões de QAT e BlueField por fonte primária; se a patente da Elasticsearch virou produto.

### 10.6 Benchmark v2: o que falta medir (prioridade)

| # | Item | Por quê |
|---|---|---|
| 1 | Rodar em **servidor-alvo** (homogêneo, 1 e 2 sockets, NUMA) e em vCPU de nuvem (c7i/EPYC) | η real e fator servidor/notebook, hoje as maiores incertezas |
| 2 | **Energia**: RAPL com permissão ou medidor de tomada; J/GB por configuração | Seção 6.4 só tem estimativa por TDP |
| 3 | **Classificador de plano**: previsão vs oráculo | **executado (10.9): não vale agora**; falta só medir features em código nativo para reabrir |
| 4 | **Dedup/CDC e delta** em VM, bancos e backups reais (o F1 cobriu só código-fonte do kernel e um par sintético), memória de `--long`, índice com BLAKE3 | componente D; F1 só parcialmente respondido |
| 5 | **I/O real**: leitura e escrita de NVMe e rede no caminho, latência p50/p99 | hoje tudo é memória/tmpfs |
| 6 | **Corpus do cliente** (logs de produção, bancos reais) | os logs são sintéticos e o dicionário foi testado na mesma fonte |
| 7 | **Neural na GPU local (RTX 5060 Laptop)**: ts_zip/NNCP/L3TC contra xz/zstd-19 com modelo contado | F2 — só depois do gate |
| 8 | **Determinismo cross-hardware** (≥4 plataformas) e **injeção de falhas** | F3 e F4 |

### 10.7 Critério de aceite ("realmente funciona")

| Condição | Estado hoje | Medido? |
|---|---|---|
| V1 vazão: 5 PB/mês (7,72 GB/s no pico) em ≤64 núcleos | **cumprida** por zstd-1/-3 (26/48 núcleos, η = 0,42, fator servidor 1,0) | sim (notebook) |
| V2 economia: valor líquido > 0 contra não comprimir | **cumprida** (US$0,63–1,21 por GB em S3 Standard, 60 meses; CPU = 0,1–6% do valor) | sim (modelo + medição) |
| V3 índice de dedup cabe na RAM | **medida só para CDC (10.8):** 8–64 KiB não cabem em 1 TB de RAM com retenção >1 mês a 5 PB/mês; 1 MiB cabe só em fluxos; o dedup de arquivo inteiro (já existente) tem índice pequeno | parcial |
| V4 exatidão: reconstrução bit-exata e determinística em ≥3 plataformas | **0 violações** em 1 plataforma; outras não testadas | parcial |
| V5 integridade: perda esperada ≤ meta com redundância fim a fim | BLAKE3 sim; *scrubbing* e erasure não | não |
| V6 latência: reconstrução ≤ SLA | descompressão zstd 1,2–3,0 GB/s por núcleo; **sem I/O real** | parcial |

**Conclusão:** com os dados atuais, a **direção clássica + planejamento econômico + camadas** cumpre V1, V2 e V4 (esta só numa plataforma) e tem lacunas conhecidas em V3, V5 e V6; a **direção neural genérica** não cumpre V1, V2 (cenário realista), V4 e V6 (Seção 9.6).

### 10.8 Rodada 2: gate F1 (dedup por bloco) e escala em núcleos homogêneos

Código: [bench/dedup.py](../bench/dedup.py) e [bench/cdc.c](../bench/cdc.c) (FastCDC compilado em tempo de execução com `gcc -O3`, chamado por `ctypes`), `--escala-cpus` em [bench/syntrabench.py](../bench/syntrabench.py) e [bench/escala_resumo.py](../bench/escala_resumo.py). Resultados: [dedup.md](../bench/resultados/dedup.md), [dedup.json](../bench/resultados/dedup.json), [escala_homogenea.md](../bench/resultados/escala_homogenea.md). Conferi no JSON os números do gate F1 que cito abaixo, a ausência de comentários no código (`.py` por tokenização, `.c` por busca) e que `medicoes.json` não foi alterado; **não reproduzi a execução**.

**Dados.** Linux kernel Par A = 6.12.114 → 6.12.115 (patch) e Par B = 6.12.115 → 6.13.12 (minor), cada versão como **árvore de arquivos** e como **fluxo tar não comprimido** (um arquivo grande, como um backup); mais um par **SINTÉTICO** (Silesia com 835 edições in-place, 40 inserções e 1% anexado). O sha256 dos três `tar.xz` confere com o `sha256sums.asc` do kernel.org; a assinatura PGP **não** foi verificada. **Chunkers:** tamanho fixo (4 KiB a 1 MiB) e FastCDC normalizado (8 KiB, 64 KiB, 1 MiB). **Sanidade (todos aprovados):** tamanho médio do chunk 1,14–1,17x o alvo (teoria do artigo: ~1,14); inserção de 100 bytes preserva ≥98,1% dos chunks no CDC e 0% no tamanho fixo; reconstrução byte a byte. **Bit-exatidão: 0 violações** em todas as estratégias. Hash: `blake2b` (BLAKE3 não está disponível), índice teórico a 32 B por entrada (**inferência própria**; metadados e receitas fora).

**Gate F1.** Baseline das árvores: dedup de arquivo inteiro + zstd-3. Baseline de tar e sintético: zstd-3 do fluxo, **sem dedup**. "Ganho extra" = redução de bytes armazenados sobre o baseline; critério do roadmap: ≥5%. Índice extrapolado a 5 PB/mês (**inferência própria**, usa a taxa de dedup de um único par): TB por mês de dado e meses de ingestão que cabem em 1 TB de RAM.

| Conjunto (razão do baseline) | CDC | % de v2 já em v1 | Razão com zstd-3 | **Ganho extra** | Índice TB/mês (meses em 1 TB) | ≥5%? |
|---|---|---:|---:|---:|---|:-:|
| **A árvore** (10,42; arquivo inteiro acha 99,17%) | 8 KiB | 99,79 | 9,05 | **−15,0%** | 10,4 (0,1) | não |
| | 64 KiB | 99,37 | 9,94 | −4,8% | 5,1 (0,2) | não |
| | 1 MiB | 99,17 | 10,34 | −0,7% | 4,7 (0,2) | não |
| **A tar** (6,96) | 8 KiB | 99,58 | 9,56 | **+27,2%** | 7,2 (0,14) | sim |
| | 64 KiB | 98,38 | 11,25 | **+38,2%** | 0,96 (1,0) | sim |
| | 1 MiB | 89,66 | 11,72 | **+40,6%** | 0,06 (15,5) | sim |
| **B árvore** (7,30; arquivo inteiro acha 66,23%) | 8 KiB | 81,10 | 7,16 | −1,9% | 12,4 (0,08) | não |
| | 64 KiB | 69,50 | 7,13 | −2,4% | 6,2 (0,16) | não |
| | 1 MiB | 67,21 | 7,27 | −0,4% | 5,8 (0,17) | não |
| **B tar** (6,97) | 8 KiB | 71,74 | 6,89 | −1,1% | 9,4 (0,11) | não |
| | 64 KiB | 48,04 | 6,79 | −2,5% | 1,5 (0,68) | não |
| | 1 MiB | 31,36 | 7,11 | +2,1% | 0,10 (9,9) | não |
| **Sintético** (3,11) | 8 KiB | 92,58 | 4,63 | **+32,8%** | 9,7 (0,10) | sim |
| | 64 KiB | 68,97 | 4,31 | **+27,8%** | 1,4 (0,72) | sim |
| | 1 MiB | 0,37 | 3,08 | −0,9% | 0,13 (7,6) | não |

O tamanho fixo é negativo em quase tudo; o melhor caso é 1 MiB no A tar, com +2,3%.

**Leitura** (as quatro primeiras são do pesquisador, conferidas contra o JSON; a quinta é minha):

1. **Em árvores de arquivos o CDC reprova.** O dedup de arquivo inteiro + zstd-3 já captura 99,2% (Par A) e 66,2% (Par B) de v2. O CDC acha mais redundância (81% no B a 8 KiB), mas **comprimir cada chunk isoladamente perde mais do que o dedup ganha** (razão do conteúdo único 5,24 por arquivo contra 4,07 com CDC 8 KiB). Ganho extra de −15% a 0%.
2. **Em fluxos únicos (tar/backup) o CDC ganha bastante sobre não deduplicar** (+27% a +41% no Par A e +28–33% no sintético), mas **reprova no Par B** (−2,5% a +2,1%), onde a similaridade é menor. O comparador é o zstd-3 do fluxo inteiro, cuja janela (2 MiB) não enxerga a versão anterior.
3. **`zstd --long=31` sobre v1+v2 concatenados, sem índice nenhum, supera o melhor CDC por chunk:** razão 14,98 (A), 14,56 (B) e 6,11 (sintético) contra 11,72, 7,11 e 4,63. Parte grande do "ganho do CDC" é recuperar redundância que uma janela longa já captura (**inferência do pesquisador**; custo de memória da janela de 2 GiB **não medido**).
4. **Delta por caminho** (limite superior de dedup por similaridade; só vale onde o nome do arquivo é conhecido): no Par A, 223 arquivos alterados custam 42,6 KB com `zstd --patch-from` contra 1,04 MB (CDC 8 KiB) e 3,33 MB (1 MiB), mas o total só cai 1,2%; no **Par B, 19.227 arquivos alterados custam 3,40 MB contra 85–122 MB com CDC, e o total fica em 286,5 MB, 29% abaixo do arquivo inteiro + zstd-3 (406,0 MB) e 31% abaixo do CDC 64 KiB (415,8 MB)**. O pesquisador descartou uma primeira versão do método (5% e 52% acima do CLI) e refez; a versão final soma 9.341 B contra 9.453 B do `--patch-from` do CLI no Par A e 4.067 contra 4.232 no Par B (40 arquivos por par), ou seja, ~1,2% e ~3,9% **abaixo** do CLI.
5. **Índice:** CDC 8 e 64 KiB **não cabem em 1 TB de RAM** a 5 PB/mês com retenção maior que 1 mês (0,08–1,04 meses de ingestão). CDC 1 MiB cabe em fluxos (10–15 meses) mas não em árvores (0,17–0,21 meses), onde o chunk equivale ao arquivo (média de 17 KiB).

**Vazão em 1 P-core (fatia de 256 MiB, mediana de 3):** FastCDC ~3,4 GB/s; blake2b 1,16–1,25 GB/s; chunker + hash + lookup em `dict` de Python ~0,9 GB/s, ou seja, ~2,2 núcleos médios e ~8,6 no pico para 5 PB/mês (**inferência**; o hash é o gargalo, BLAKE3 não foi medido e o `dict` não representa um índice de produção).

**Efeito sobre a recomendação (inferência minha):** o componente D da Seção 10.4 deixa de ser "CDC hierárquico". Para **árvores de arquivos**, manter dedup de arquivo inteiro. Para **famílias de versões** e **fluxos de backup**, o que mais rendeu foi **referência entre versões** (`--patch-from`, janela longa), não chunk dedup. CDC com chunks ≥1 MiB só se justifica em fluxos únicos sem estrutura de arquivo. **Limite forte:** só foram testados código-fonte do kernel e um par sintético; **VM, bancos e backups reais, onde o CDC costuma render mais, não foram medidos**.

**Escala em núcleos homogêneos** (zstd-3 e zstd-1, blocos de 1 MiB, processos independentes; eficiência relativa a 1 núcleo do **mesmo tipo**)

| Configuração | N | zstd-3 compressão MB/s | eficiência zstd-3 | eficiência zstd-1 |
|---|---:|---:|---:|---:|
| só P-cores físicos | 8 | 3.196 | 0,89 | 0,91 |
| só E-cores | 4 | 806 | 0,74 | 0,89 |
| só E-cores | 16 | 3.128 | **0,72** | **0,88** |
| P-cores com SMT (8 físicos + 8 irmãos) | 16 | 3.729 | **0,51** | 0,54 |
| 8 P + 8 E | 16 | 4.310 | 0,74 (0,59 vs P-core isolado) | — |

Vazão isolada de 1 núcleo: P 463 / E 275 MB/s em compressão (E/P = 0,59) e 1.904 / 1.205 em descompressão (0,63). O SMT soma +17% (compressão) e +10% (descompressão) sobre 8 núcleos físicos: 16 threads valem ~9,3 núcleos físicos. Nos E-cores a queda se concentra entre N=2 e N=4 e depois fica plana; os E-cores formam clusters de 4 que dividem 4 MiB de L2 (confirmado em `/sys`), o que **sugere** limite de L2 de cluster, e não de banda de memória global (**inferência sem controle**: não testei E-cores espalhados um por cluster).

**Conclusão (inferência):** a queda da curva anterior (0,64 a 16; 0,42 a 32) **não é só artefato de híbrido**: com 16 E-cores homogêneos a eficiência é 0,72 (zstd-3) e 0,88 (zstd-1). Atribuição multiplicativa a 16 threads: ~60% híbrido/SMT e ~40% limite de sistema. O 0,64 original usava uma referência isolada ~8% pessimista (N=1 dava 1,08): 0,64/1,08 ≈ 0,59, igual ao valor atual. **Um servidor homogêneo deve ficar em ~0,8–0,9 entre 8 e 16 núcleos; acima de 16 núcleos não há dado.** Com η = 0,85, o N de pico para 5 PB/mês seria ~15 (zstd-1) e ~24 (zstd-3), contra 26 e 48 com o η = 0,42 do híbrido.

**Falhas e limites desta rodada:** a primeira extração em paralelo estourou a quota do `/tmp` (`usrquota`, ~7,5 GiB reais apesar de o `df` mostrar 11 GB) e travou até o shell; o preparo foi refeito extraindo uma versão por vez, e a "árvore" da análise é lida do tar por offset com sha256 de cada arquivo conferido contra a árvore antes de apagá-la (desvio da letra do pedido, registrado nos dados). A análise foi rodada duas vezes por engano; `dedup.json` vem da primeira execução completa. zstd-19 só por amostra de 1/4 dos chunks únicos, sem baseline de arquivo inteiro com zstd-19. A referência isolada de 1 núcleo tem ruído de ~8% entre rodadas. Energia continua não medida. O tempo total provavelmente excedeu os 60 min pedidos (as fases de medição somam ~11 min).

### 10.9 Rodada 3: classificador de plano por bloco (T1.2)

Código: [bench/plano_bloco.py](../bench/plano_bloco.py) (subcomandos `autoteste`, `kernel`, `rotular`, `features`, `custo-transform`, `avaliar`, `tempo`, `relatorio`); resultados em [plano_bloco.md](../bench/resultados/plano_bloco.md) e [plano_bloco.json](../bench/resultados/plano_bloco.json). Pergunta: **um classificador barato (µs–ms) escolhe, por bloco, o melhor plano (transform + codec + nível) melhor que um plano fixo ou um *probe* simples?** O plano fica no envelope; a rede/modelo ficaria só no plano de controle.

**Dados e verificação.** 34 arquivos/grupos, 574,7 MB: Silesia, Canterbury, 3 classes sintéticas (incompressível, logs, já comprimido) e uma classe `código kernel` (primeiros 256 MiB do tar de `linux-6.12.115.tar.xz` lidos em streaming: 242,6 MB, 42% dos bytes, em 16 pseudo-arquivos por diretório; o sha256 do `tar.xz` inteiro não foi conferido). Os dados foram **regenerados** porque a máquina reiniciou e o tmpfs foi limpo (sha256 dos 18 arquivos conferido contra `medicoes.json`). Blocos independentes de 64 KiB (8.782), 256 KiB (2.207) e 1 MiB (562). **139.652 comparações bloco × candidato com decodificação e sha256: 0 falhas**; os transforms foram conferidos contra uma implementação ingênua da semântica de `src/transform.rs` em 846 casos de borda. Candidatos síncronos: `stored`, `zstd-1`, `zstd-3`, `delta` (1/2/4/8) e `byte_split` (4/8) seguidos de `zstd-3`.

**Regret de bytes contra o oráculo de tamanho** (leave-one-file-out, global; menor é melhor):

| Plano | 64 KiB | 256 KiB | 1 MiB |
|---|---:|---:|---:|
| **`zstd-3` fixo** | **1,52%** | **1,58%** | **1,12%** |
| `zstd-1` fixo | 4,78% | 4,99% | 7,63% |
| *probe* + `zstd-3` | 1,59% | 1,72% | 1,41% |
| regra manual | 3,03% | 4,11% | 3,35% |
| CART (profundidade 6) | 0,62% | 1,66% | 0,47% |
| regressão logística | 0,39% | 0,76% | 0,80% |

**Critério de parada** (declarado antes do experimento: (i) regret ≤1%, (ii) ganho ≥1% sobre *probe* + `zstd-3`, (iii) features + inferência ≤2% do tempo do `zstd-3`):

| Granularidade (melhor modelo) | (i) | (ii) | (iii) medido em numpy / **estimado nativo** | Veredito |
|---|---|---|---|---|
| 64 KiB (logística) | 0,39% [IC95 0,15–0,70] atende | 1,18% [IC95 0,07–3,42] atende | 123,6% / 37,0% **falha** | **não vale** |
| 256 KiB (logística) | 0,76% atende | 0,95% [IC95 0,12–2,59] **falha** | 31,2% / 8,8% **falha** | **não vale** |
| 1 MiB (CART) | 0,47% [IC95 0,11–1,05] atende | 0,92% [IC95 −0,02–2,80] **falha por 0,08 p.p.** | 3,2% **falha** / 1,4% (atenderia; **inferência**) | **não vale** com o medido |

O *probe* fica a 1,41–1,72% do oráculo, então a regra "usar só o *probe*" também não dispara; o veredito vem das condições (ii) e (iii). Os intervalos de confiança (*bootstrap* por arquivo) têm 1 a 3 pontos de largura: o resultado de 1 MiB está no limite.

**Custo (numpy, 1 núcleo, mediana de 3 por bloco):** `zstd-3` 121 µs (64 KiB), 516 µs (256 KiB), 1.912 µs (1 MiB); features completas 147–158 µs; features do CART 110/123/60 µs; inferência 0,35 µs (CART) e 2,8–3,0 µs (logística). O CART tem 73, 51 e 23 nós; a logística, 198 pesos. A estimativa nativa (**inferência própria**: ~2 GB/s por passada sobre a amostra) dá 30%, 7,6% e 1,4% do `zstd-3` para o CART em 64 KiB, 256 KiB e 1 MiB. Em classes incompressíveis o `zstd-3` já detecta em ~150 µs.

**O que o experimento mostra de útil:**

1. **O teto é pequeno:** qualquer seletor só pode ganhar o que sobra do `zstd-3` fixo: **1,1–1,6% dos bytes**. Em texto, banco, XML e kernel ele já está a menos de 0,7% do oráculo.
2. **O valor está concentrado em dados numéricos:** a perda do `zstd-3` fixo é de **11–13% em `científico`** e 3,4–3,6% em `dna` (64 e 256 KiB). Escolher o **melhor transform por bloco** ganha **9,4–11,6%** sobre o `zstd-3` em `científico`; um transform **fixo** por classe perde 1,0–3,5% (`byte_split 4`). Código ganha 0,4–1,2% e binário 0–0,8% com transforms; o resto ≈ 0.
3. **Custo de vazão dos transforms (numpy):** a compressão cai de 348 para 325 MB/s e a descompressão de 1.412 para 786 MB/s, porque a inversão em numpy é lenta (delta de largura 1: 0,73 GB/s); a implementação nativa do motor não deve ter essa penalidade (**inferência**, não medida).
4. **Rótulo econômico:** treinar com o oráculo econômico (valor líquido) não melhorou nada sobre o oráculo de tamanho.
5. **Medir todos os candidatos** custa 8x o tempo do `zstd-3` com os 9 candidatos síncronos e 173x com 14: é o que o planner atual faz, e é inviável no caminho síncrono.
6. **Generalização** (leave-one-class-out): o regret global sobe para 0,56–2,92%; DNA, código, XML, banco e logs têm 1 arquivo cada, então o resultado dessas classes já equivale a "classe nova".

**Decisão e efeito (inferência minha, apoiada nos números):** **não implementar classificador aprendido agora.** O componente B passa a ser um **planejador simples**: *probe* de incompressibilidade + `zstd-3` por bloco + **seleção de transform por amostra só em blocos numéricos** (detectados pela entropia dos deltas), tudo em código nativo. Isso reforça o T1.9 (OpenZL), que ataca exatamente dados estruturados/numéricos, onde está o ganho. **Reabrir o classificador** se (a) features nativas medirem ≤2% do `zstd-3` e (b) o IC do ganho sobre o *probe* ficar acima de 1% com dados do cliente.

**Limites:** o "não vale" depende do custo em numpy (a estimativa nativa é inferência); 18 arquivos reais (a validação por arquivo é grossa) e IC de 1–3 pontos; as vazões por classe vêm do benchmark anterior e o kernel usa a de `código` (inferência); o classificador do caminho assíncrono **não foi treinado** (só o oráculo assíncrono, 14–19% menor que o síncrono); o limiar de 3% para tentar transform + xz, a tolerância de 0,3% nos rótulos e a margem de 0,5 bit da regra manual são escolhas do pesquisador. Excedeu o orçamento de 60 min (a regeneração dos dados e o desenvolvimento; o cálculo soma ~5 min).

---

## Bibliografia

**Como ler os rótulos.** **[C]** metadados conferidos por mim no Crossref (2026-11-03). **[V]** número lido na fonte por um pesquisador desta sessão (os subagentes baixaram PDFs/HTML; eu não reabri cada um). **[E]** existência conferida por mim em busca independente. **[NV]** de memória ou só resumo de busca. Referências de 2026 estão **fora do meu conhecimento prévio**: só PMATIC e Nacrith tiveram existência confirmada por mim; **confira as demais antes de citar externamente**. Onde um pesquisador sinalizou inconsistência (Delétang abstract × Tabela 1; LMCompress; relatório JVET em rascunho; Llama 3, percentuais), isso está dito no corpo.

### Teoria (Seção 2)
- Shannon, C. E. (1948). *A Mathematical Theory of Communication*. Bell System Technical Journal 27(3–4). DOI [10.1002/j.1538-7305.1948.tb01338.x](https://doi.org/10.1002/j.1538-7305.1948.tb01338.x) **[C]**
- Shannon, C. E. (1951). *Prediction and Entropy of Printed English*. BSTJ 30(1):50–64. DOI [10.1002/j.1538-7305.1951.tb01366.x](https://doi.org/10.1002/j.1538-7305.1951.tb01366.x) **[C]**
- Shannon, C. E. (1959). *Coding theorems for a discrete source with a fidelity criterion*. IRE National Convention Record, Part 4:142–163. **[NV]**
- Kolmogorov, A. N. (1965). *Three approaches to the quantitative definition of information*. Problems of Information Transmission 1(1):1–7. **[NV]**
- Solomonoff, R. J. (1964), *A formal theory of inductive inference*, Information and Control 7; Chaitin, G. J. (1966), *On the length of programs for computing finite binary sequences*, JACM 13. **[NV]**
- Li, M., Vitányi, P. (2019). *An Introduction to Kolmogorov Complexity and Its Applications*, 4ª ed., Springer. DOI [10.1007/978-3-030-11298-1](https://doi.org/10.1007/978-3-030-11298-1) **[C]**
- Witten, I. H., Neal, R. M., Cleary, J. G. (1987). *Arithmetic coding for data compression*. CACM 30(6):520–540. DOI [10.1145/214762.214771](https://doi.org/10.1145/214762.214771) **[C]**
- Rissanen, J., Langdon, G. G. (1979). *Arithmetic coding*. IBM J. Res. Dev. 23(2):149–162. **[NV]**
- Rissanen, J. (1978). *Modeling by shortest data description*. Automatica 14(5):465–471. DOI [10.1016/0005-1098(78)90005-5](https://doi.org/10.1016/0005-1098(78)90005-5) **[C]**
- Grünwald, P. D. (2007). *The Minimum Description Length Principle*. MIT Press. **[NV]**
- Cover, T. M., Thomas, J. A. (2006). *Elements of Information Theory*, 2ª ed., Wiley. **[NV]**
- Blau, Y., Michaeli, T. (2019). *Rethinking Lossy Compression: The Rate-Distortion-Perception Tradeoff*. ICML 2019. [arXiv:1901.07821](https://arxiv.org/abs/1901.07821) **[V]**

### Compressão neural visual e implícita (Seção 3.1–3.3, 3.6)
- Ballé, J., Laparra, V., Simoncelli, E. P. (2017). *End-to-end Optimized Image Compression*. ICLR 2017. [arXiv:1611.01704](https://arxiv.org/abs/1611.01704) **[V]**
- Ballé, J., Minnen, D., Singh, S., Hwang, S. J., Johnston, N. (2018). *Variational image compression with a scale hyperprior*. ICLR 2018. [arXiv:1802.01436](https://arxiv.org/abs/1802.01436) **[V]**
- Minnen, D., Ballé, J., Toderici, G. (2018). *Joint Autoregressive and Hierarchical Priors for Learned Image Compression*. NeurIPS 2018. [arXiv:1809.02736](https://arxiv.org/abs/1809.02736) **[V]**
- He, D. et al. (2022). *ELIC: Efficient Learned Image Compression with Unevenly Grouped Space-Channel Contextual Adaptive Coding*. CVPR 2022. [arXiv:2203.10886](https://arxiv.org/abs/2203.10886) **[V]**
- Ballé, J., Johnston, N., Minnen, D. (2019). *Integer Networks for Data Compression with Latent-Variable Models*. ICLR 2019. [Google Research](https://research.google/pubs/integer-networks-for-data-compression-with-latent-variable-models/) **[V]** (abstract)
- Esenlik, A. et al. (2025). *An Overview of the JPEG AI Learning-Based Image Coding Standard*. [arXiv:2510.13867](https://arxiv.org/abs/2510.13867); Jia, Z. et al. (2025). *Overview of Variable Rate Coding in JPEG AI*. [arXiv:2503.16288](https://arxiv.org/abs/2503.16288); JPEG Committee (2024-05-07), [press release](https://jpeg.org/items/20240507_press.html); ISO/IEC 6048-1:2025 / ITU-T T.840.1, [catálogo ISO](https://www.iso.org/standard/88911.html) (texto normativo de decodificação **não lido**). **[V]**
- Koyuncu, A. B. et al. (2024). *Quantized Decoder in Learned Image Compression for Deterministic Reconstruction*. ICASSP 2024. [arXiv:2312.11209](https://arxiv.org/abs/2312.11209) **[V]**
- Li, J., Li, B., Lu, Y. (2023/2024). *DCVC-DC* ([arXiv:2302.14402](https://arxiv.org/abs/2302.14402)); *DCVC-FM* ([arXiv:2402.17414](https://arxiv.org/abs/2402.17414)). Jia, Z. et al. (2025). *Towards Practical Real-Time Neural Video Compression (DCVC-RT)*. CVPR 2025. [arXiv:2502.20762](https://arxiv.org/abs/2502.20762) **[V]**
- Teng, R. et al. (2024). *Benchmarking Conventional and Learned Video Codecs with a Low-Delay Configuration*. [arXiv:2408.05042](https://arxiv.org/abs/2408.05042); Li, Y. et al. (2023). *Designs and Implementations in Neural Network-based Video Coding*. [arXiv:2309.05846](https://arxiv.org/abs/2309.05846); JVET (2026), relatório da 43ª reunião (rascunho com *placeholders*), [docx](https://www.itu.int/wftp3/AV-Arch/jvet-site/2026_07_AQ_Geneva/JVET-AQ_notes_d0.docx) **[V]/[NV]**
- Dupont, E. et al. (2021). *COIN* [arXiv:2103.03123](https://arxiv.org/abs/2103.03123); (2022). *COIN++* TMLR [arXiv:2201.12904](https://arxiv.org/abs/2201.12904); Sitzmann, V. et al. (2020). *SIREN* [arXiv:2006.09661](https://arxiv.org/abs/2006.09661); Chen, H. et al. (2021). *NeRV* NeurIPS [arXiv:2110.13903](https://arxiv.org/abs/2110.13903); Kim, H. et al. (2023). *C3* [arXiv:2312.02753](https://arxiv.org/abs/2312.02753); Ladune, T. et al. (2026). *Cool-chic 5.0* [arXiv:2605.02726](https://arxiv.org/abs/2605.02726) **[V]**
- Mentzer, F. et al. (2019). *L3C* CVPR [arXiv:1811.12817](https://arxiv.org/abs/1811.12817); Hoogeboom, E. et al. (2019). *IDF* NeurIPS [arXiv:1905.07376](https://arxiv.org/abs/1905.07376); Zhang, S. et al. (2021). *iVPF* CVPR [arXiv:2103.16211](https://arxiv.org/abs/2103.16211); Townsend, J. et al. (2019). *BB-ANS* ICLR [arXiv:1901.04866](https://arxiv.org/abs/1901.04866), *HiLLoC* [arXiv:1912.09953](https://arxiv.org/abs/1912.09953); Bai, Y. et al. *DLPR* TPAMI [arXiv:2209.04847](https://arxiv.org/abs/2209.04847) **[V]**
- Zeghidour, N. et al. (2021). *SoundStream* [arXiv:2107.03312](https://arxiv.org/abs/2107.03312); Défossez, A. et al. (2022). *EnCodec* [arXiv:2210.13438](https://arxiv.org/abs/2210.13438); Kumar, R. et al. (2023). *DAC* NeurIPS [arXiv:2306.06546](https://arxiv.org/abs/2306.06546); Long, Novack, Donahue (2026). *Benchmarking Language Modeling for Lossless Compression of Full-Fidelity Audio*, Interspeech 2026 [arXiv:2603.08683](https://arxiv.org/abs/2603.08683) **[V]**
- Mentzer, F. et al. (2020). *HiFiC* NeurIPS [arXiv:2006.09965](https://arxiv.org/abs/2006.09965); Theis, L. et al. (2022). *DiffC* [arXiv:2206.08889](https://arxiv.org/abs/2206.08889); Hofer, Böhme (2024). *A Taxonomy of Miscompressions*, WIFS [arXiv:2409.05490](https://arxiv.org/abs/2409.05490); Kriesel, D. (2013). *Xerox WorkCentres are switching written numbers when scanning* [blog](https://www.dkriesel.com/en/blog/2013/0802_xerox-workcentres_are_switching_written_numbers_when_scanning); Wang et al. (2026). *LatentStore* [arXiv:2605.19385](https://arxiv.org/abs/2605.19385) **[V]**
- Horn, D. R., Elkabany, K., Lesniewski-Laas, C., Winstein, K. (2017). *The Design, Implementation, and Deployment of a System to Transparently Compress Hundreds of Petabytes of Image Files for a File-Storage Service* (Lepton). NSDI '17. [arXiv:1704.06192](https://arxiv.org/abs/1704.06192) **[E]** (≈23% de média; 46 de 203 PiB)

### Compressão neural de dados genéricos (Seção 3.4–3.5, 3.7)
- Delétang, G. et al. (2024). *Language Modeling Is Compression*. ICLR 2024. [arXiv:2309.10668](https://arxiv.org/abs/2309.10668) **[V]**
- Bellard, F. (2019/2021). *NNCP* [site](https://bellard.org/nncp/), [pdf v1](https://bellard.org/nncp/nncp.pdf), [pdf v2.1](https://bellard.org/nncp/nncp_v2.1.pdf); *ts_zip* [site](https://bellard.org/ts_zip/) **[V]**
- Mahoney, M. *Large Text Compression Benchmark* [site](https://www.mattmahoney.net/dc/text.html); Knoll, B. *cmix* [site](http://www.byronknoll.com/cmix.html), [GitHub](https://github.com/byronknoll/cmix) **[V]** (linhas de 2026 **não reconfirmadas** por mim)
- Valmeekam, C. S. K. et al. (2023). *LLMZip* [arXiv:2306.04050](https://arxiv.org/abs/2306.04050); Mittu, F. et al. (2024). *FineZip* [arXiv:2409.17141](https://arxiv.org/abs/2409.17141); Li, Z. et al. (2025). *Lossless data compression by large models (LMCompress)*, Nat. Mach. Intell. 7:794–799, DOI [10.1038/s42256-025-01033-7](https://doi.org/10.1038/s42256-025-01033-7), [arXiv:2407.07723](https://arxiv.org/abs/2407.07723); Heurtel-Depeiges, D. et al. (2024/25). *Compression via Pre-trained Transformers: A Study on Byte-Level Multimodal Data* [arXiv:2410.05078](https://arxiv.org/abs/2410.05078); Zhang, J. et al. (2025). *L3TC* AAAI [arXiv:2412.16642](https://arxiv.org/abs/2412.16642) **[V]**
- Adler, A., Tang, J. (2026). *Synchronizing Probabilities in Model-Driven Lossless Compression (PMATIC)*. ICLR 2026. [arXiv:2601.10678](https://arxiv.org/abs/2601.10678) **[V][E]**; Armstrong et al. (2026). *Hybrid-Neural Archival* [arXiv:2603.25526](https://arxiv.org/abs/2603.25526) **[V]**; Tacconelli, R. (2026). *Nacrith* [arXiv:2602.19626](https://arxiv.org/abs/2602.19626) **[V][E]**; Nardone, Ferragina (2026). *LLM-based Source Code Compression via Thresholded Symbol Ranking* [arXiv:2607.24192](https://arxiv.org/abs/2607.24192) **[V]**
- Goyal, M. et al. (2020). *DZip* [arXiv:1911.03572](https://arxiv.org/abs/1911.03572); Mao, Y. et al. (2022). *TRACE*, WWW 2022 [arXiv:2203.16114](https://arxiv.org/abs/2203.16114) (ordem dos autores não confirmada); Ma et al. (2026). *FADE*, ACL 2026 [arXiv:2604.07239](https://arxiv.org/abs/2604.07239) **[V]**
- Abbas, A. et al. (2023). *SemDeDup* [arXiv:2303.09540](https://arxiv.org/abs/2303.09540); Lee, K. et al. (2022). *Deduplicating Training Data Makes Language Models Better*, ACL [arXiv:2107.06499](https://arxiv.org/abs/2107.06499); Jiang, H. et al. (2023). *LLMLingua*, EMNLP [arXiv:2310.05736](https://arxiv.org/abs/2310.05736); Broder, A. (1997). *On the resemblance and containment of documents*, DOI [10.1109/SEQUEN.1997.666900](https://doi.org/10.1109/SEQUEN.1997.666900) **[V]/[NV]**

### Armazenamento, custos e energia (Seção 4)
- Zhu, B., Li, K., Patterson, H. (2008). *Avoiding the Disk Bottleneck in the Data Domain Deduplication File System*. FAST '08. [PDF](https://www.usenix.org/legacy/event/fast08/tech/full_papers/zhu/zhu.pdf) **[V]**
- Meyer, D. T., Bolosky, W. J. (2011). *A Study of Practical Deduplication*. FAST '11. [PDF](http://www.usenix.org/events/fast11/tech/full_papers/Meyer.pdf) **[V]**
- Wallace, G. et al. (2012). *Characteristics of Backup Workloads in Production Systems*. FAST '12. [PDF](https://www.usenix.org/system/files/conference/fast12/wallace.pdf) **[V]**
- Xia, W. et al. (2016). *FastCDC*. USENIX ATC '16. [PDF](https://www.usenix.org/system/files/conference/atc16/atc16-paper-xia.pdf) **[V]**
- Huang, C. et al. (2012). *Erasure Coding in Windows Azure Storage*. USENIX ATC '12. [PDF](https://www.usenix.org/system/files/conference/atc12/atc12-final181_0.pdf) **[V]**
- Lillibridge, M. et al. (2009). *Sparse Indexing: Large Scale, Inline Deduplication Using Sampling and Locality*. FAST '09. **[NV]**
- Documentação: [OpenZFS](https://openzfs.github.io/openzfs-docs/man/master/7/zfsconcepts.7.html), [TrueNAS dedup](https://truenas.com/docs/references/zfsdeduplication), [Ceph BlueStore](https://docs.ceph.com/en/latest/rados/configuration/bluestore-config-ref/), [Facebook zstd](https://github.com/facebook/zstd) **[V]**; [Ceph dedup](https://docs.ceph.com/en/latest/dev/deduplication/) **[NV]**
- Backblaze: [Vault architecture (2019)](https://www.backblaze.com/blog/vault-cloud-storage-architecture/), [Cloud storage durability (2018)](https://www.backblaze.com/blog/cloud-storage-durability/), [Drive Stats 2025 (fev/2026)](https://www.backblaze.com/blog/backblaze-drive-stats-for-2025/) **[V]**; tabela por modelo via [StorageReview](https://www.storagereview.com/news/backblaze-2025-year-end-drive-stats-annual-afr-falls-to-1-36-as-high-capacity-drives-dominate-fleet) **[S]**
- Preços: [AWS S3](https://b0.p.awsstatic.com/pricing/2.0/meteredUnitMaps/s3/USD/current/s3.json) e [transferência](https://b0.p.awsstatic.com/pricing/2.0/meteredUnitMaps/datatransfer/USD/current/datatransfer.json), [B2](https://www.backblaze.com/cloud-storage/pricing), [R2](https://developers.cloudflare.com/r2/pricing/), [Lambda](https://lambda.ai/pricing), [Vantage/EC2](https://instances.vantage.sh/aws/ec2/p5.48xlarge), [TrendForce 02/02/2026](https://www.trendforce.com/presscenter/news/20260202-12911.html), [03/07/2026](https://www.trendforce.com/presscenter/news/20260703-13134.html), [30/09/2026](https://www.trendforce.com/presscenter/news/20260930-13258.html) **[V]**; HDD/SSD de varejo e WD ASP **[S]** (eRacks, HDDHunt, capitalandcompute, pbxscience)
- Datasheets: [WD Ultrastar HC590](https://documents.westerndigital.com/content/dam/doc-library/en_us/assets/public/western-digital/product/data-center-drives/ultrastar-dc-hc500-series/data-sheet-ultrastar-dc-hc590.pdf), [Seagate Exos X24](https://www.seagate.com/content/dam/seagate/en/content-fragments/products/datasheets/exos-x24/exos-x24-DS2080-2307US-en_US.pdf), [Solidigm D5-P5336](https://www.solidigm.com/content/dam/solidigm/en/site/products/technology/p5336-product-brief/documents/solidigm-d5-p5336-product-brief.pdf), [LTO-10](https://www.lto.org/lto-10/) **[V]**
- Energia: Tannu, S., Nair, P. J. (2022/2023). *The Dirty Secret of SSDs: Embodied Carbon*. HotCarbon 2022. [arXiv:2207.10793](https://arxiv.org/abs/2207.10793) **[V]** (contestado em 2025); CERN (2026). *Environmental efficiency of tape storage at CERN* (poster SC4RC) [PDF](https://indico.cern.ch/event/1526482/contributions/7027775/attachments/3267151/5835421/Environmental%20efficiency%20of%20tape%20storage%20at%20CERN%20-%20SC4RC%202026%20-%20poster.pdf) **[V]**; Elsworth, C. et al. (2025). *Measuring the environmental impact of delivering AI at Google Scale* [arXiv:2508.15734](https://arxiv.org/abs/2508.15734) **[V]**; Luccioni, A. S. et al. (2024). *Power Hungry Processing*, FAccT [arXiv:2311.16863](https://arxiv.org/abs/2311.16863) **[V]**
- Agregados: Uptime Institute (2026), [Global Data Center Survey 2026](https://intelligence.uptimeinstitute.com/resource/growing-pue-advantage-larger-data-centers) **[V]**; LBNL (dez/2024), [relatório](https://eta-publications.lbl.gov/publications/2024-lbnl-data-center-energy-usage-report) via [DOE](https://www.energy.gov/articles/doe-releases-new-report-evaluating-increase-electricity-demand-data-centers) **[V]**; IEA (2025), [*Energy and AI*](https://www.iea.org/reports/energy-and-ai) **[NV]** (415 TWh de 2024)

### Falhas, corrupção e determinismo (Seção 5)
- Bairavasundaram, L. N. et al. (2008). *An Analysis of Data Corruption in the Storage Stack*. FAST '08. [PDF](https://www.usenix.org/legacy/event/fast08/tech/full_papers/bairavasundaram/bairavasundaram.pdf); (2007). *An Analysis of Latent Sector Errors in Disk Drives*. SIGMETRICS '07. [PDF](https://pages.cs.wisc.edu/~remzi/Classes/739/Fall2015/Papers/latent-sigmetrics07.pdf) **[V]**
- Schroeder, B., Gibson, G. A. (2007). *Disk Failures in the Real World*. FAST '07. [PDF](https://www.cs.utoronto.ca/~bianca/papers/fast07.pdf); Pinheiro, E., Weber, W.-D., Barroso, L. A. (2007). *Failure Trends in a Large Disk Drive Population*. FAST '07. [Google](https://research.google/pubs/failure-trends-in-a-large-disk-drive-population/); Schroeder, B., Lagisetty, R., Merchant, A. (2016). *Flash Reliability in Production*. FAST '16. [USENIX](https://www.usenix.org/conference/fast16/technical-sessions/presentation/schroeder); Zhang, Y. et al. (2010). *End-to-end Data Integrity for File Systems: A ZFS Case Study*. FAST '10. [PDF](https://www.usenix.org/legacy/event/fast10/tech/full_papers/zhang.pdf) **[V]**
- Schroeder, B., Pinheiro, E., Weber, W.-D. (2009). *DRAM Errors in the Wild*. SIGMETRICS '09. [PDF](https://www.cs.toronto.edu/~bianca/papers/sigmetrics09.pdf); Hwang, A. A., Stefanovici, I., Schroeder, B. (2012). *Cosmic Rays Don't Strike Twice*. ASPLOS '12. [PDF](https://www.cs.utoronto.ca/~bianca/papers/ASPLOS2012.pdf) **[V]**
- Hochschild, P. et al. (2021). *Cores that don't count*. HotOS '21. [PDF](https://sigops.org/s/conferences/hotos/2021/papers/hotos21-s01-hochschild.pdf); Dixit, H. D. et al. (2021). *Silent Data Corruptions at Scale* [arXiv:2102.11245](https://arxiv.org/abs/2102.11245); (2022). *Detecting Silent Data Corruptions in the Wild* [arXiv:2203.08989](https://arxiv.org/abs/2203.08989); Wang, S. et al. (2024). *Understanding Silent Data Corruption in Processors for Mitigating its Effects*, ACM TACO, DOI [10.1145/3690825](https://doi.org/10.1145/3690825) (taxa exata **[NV]**); Llama Team, Meta (2024). *The Llama 3 Herd of Models* [arXiv:2407.21783](https://arxiv.org/abs/2407.21783); Gemini Team (2023) [arXiv:2312.11805](https://arxiv.org/abs/2312.11805); Ma et al. (2025). *Understanding Silent Data Corruption in LLM Training* [arXiv:2502.12340](https://arxiv.org/abs/2502.12340); Google (2022). *Detection and Prevention of SDC in an Exabyte-scale Database System* [Google](https://research.google/pubs/detection-and-prevention-of-silent-data-corruption-in-an-exabyte-scale-database-system/); OCP (2025). *Silent Data Corruption in AI* [whitepaper](https://www.opencompute.org/documents/sdc-in-ai-ocp-whitepaper-final-pdf) **[V]**
- NVIDIA: [cuBLAS reproducibility](https://docs.nvidia.com/cuda/cublas/index.html#results-reproducibility), [cuDNN reproducibility](https://docs.nvidia.com/deeplearning/cudnn/backend/v9.19.0/developer/misc.html), [Floating Point and IEEE 754 Compliance](https://docs.nvidia.com/cuda/floating-point/index.html); [PyTorch Reproducibility](https://docs.pytorch.org/docs/2.9/notes/randomness.html); He, H. et al. (2025). *Defeating Nondeterminism in LLM Inference*, Thinking Machines [blog](https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/); Yuan et al. (2025) [arXiv:2506.09501](https://arxiv.org/abs/2506.09501); Atil et al. (2025). *Non-Determinism of "Deterministic" LLM Settings* [arXiv:2408.04667](https://arxiv.org/abs/2408.04667); Badash et al. (2026). *Hawkeye*, MLSys [arXiv:2603.20421](https://arxiv.org/abs/2603.20421); Tian et al. (2023) [arXiv:2309.11276](https://arxiv.org/abs/2309.11276) **[V]/[V\*]**
- Formatos e RFCs: Collet, Y., Kucherawy, M. (2021). [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878) (Zstandard); [RFC 9842](https://www.rfc-editor.org/rfc/rfc9842) (Compression Dictionary Transport); Alakuijala, J., Szabadka, Z. (2016). [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932) (Brotli); [RFC 3385](https://www.rfc-editor.org/rfc/rfc3385) (iSCSI CRC); [especificação xz](https://github.com/tukaani-project/xz/blob/master/doc/xz-file-format.txt); [manual bzip2](https://sourceware.org/bzip2/manual/manual.html); [xxHash](https://github.com/Cyan4973/xxHash); [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) **[V]**
- Spiteri, Buttigieg (2010). *Arithmetic Coding for Joint Source-Channel Coding*. [PDF](https://www.scitepress.org/Papers/2010/29444/29444.pdf) **[V]**; Boyd, C. et al. (1997). *Integrating error detection into arithmetic coding*, IEEE Trans. Commun. 45(1), DOI [10.1109/26.554275](https://doi.org/10.1109/26.554275) **[NV]**
- Ruggiero, P., Heckathorn, M. (2012). *Data Backup Options*, US-CERT/CISA. [PDF](https://www.cisa.gov/sites/default/files/publications/data_backup_options.pdf) **[V]**; Hugging Face forum (03/07/2025) sobre o 404 de `runwayml/stable-diffusion-v1-5`, [thread](https://discuss.huggingface.co/t/license-agreement-error-runwayml-stable-diffusion-v1-5-returns-404/161673) **[NV]**

### Itens que **não encontrei** (lacunas declaradas pelos pesquisadores)
Acumulado oficial de preços DRAM/NAND desde 2025; preço spot de GPU no Google/Azure; potência de drive LTO-9/10; estudo Seagate/IDC de Wh/TB; participação do armazenamento no consumo de data centers (IEA/LBNL); taxa de SDC por mil GPUs; número exato do estudo Alibaba; texto normativo do JPEG AI; estudo específico de perda de modelo em compressão neural; benchmark neural direto contra zstd/xz em logs/SQL/séries; throughput do artigo Delétang; DCVC vs AV1.


---

