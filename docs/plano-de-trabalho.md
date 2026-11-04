# Plano de trabalho: Syntra Engine em linha de vazão (5 PB/mês)

Data: 2026-11-04 · Base: [docs/pesquisa-compressao-neural.md](pesquisa-compressao-neural.md) e [bench/](../bench/) · Estimativas de prazo são **minhas, sem fonte** (1 desenvolvedor + agentes de apoio) e devem ser recalibradas ao fim da Fase 1.

## 1. Objetivo e critério de sucesso

Construir, a partir do motor atual, um **caminho de dados clássico em linha de vazão**, com **planejamento econômico por bloco**, **recompressão assíncrona em camadas**, **referência entre versões** e **integridade provada**, capaz de processar **5 PB/mês** (1,93 GB/s médios; pico ×4 = 7,72 GB/s) no I/O de um data center, com reconstrução **bit-exata**. Cenário-alvo (D1, confirmado em 2026-11-04): uma empresa cliente com 5 PB/mês de processamento que armazena "um pouco menos que isso" (assumo retenção de 90%), cujo ganho esperado é **reduzir o custo de processamento e de armazenamento**. A rede neural fica **fora do caminho de dados**; só atua no plano de controle e numa camada fria opcional, congelada atrás de gates (Seção 6 da pesquisa, Seção 8).

O trabalho está **pronto** quando as seis condições abaixo forem cumpridas em hardware de servidor e com dados do piloto:

| Cond. | Meta numérica | Como medir | Estado em 2026-11-04 | Tarefas |
|---|---|---|---|---|
| **V1** vazão | 5 PB/mês em ≤64 núcleos no pico ×4: zstd-3 por bloco ≥ 287 MB/s/núcleo com η = 0,42, ou ≥ 142 MB/s com η = 0,85 | `bench/syntrabench.py` e o motor sob carga, em servidor homogêneo | **cumprida em notebook** (zstd-3: 378 MB/s/núcleo; 48 núcleos de pico com η = 0,42). Falta servidor e η acima de 16 núcleos | T1.7, T2.4 |
| **V2** economia | valor líquido > 0 contra não comprimir em toda classe; planejador por bloco ≥ zstd-3 fixo | `bench/economia.py` | **cumprida** (US$0,63–1,21/GB em S3 Standard, 60 meses) | T1.2, T3.1 |
| **V3** índice | índice de dedup ≤ RAM prevista (parâmetro do piloto); dedup de arquivo inteiro como base | `bench/dedup.py` + motor | **parcial**: CDC 8–64 KiB não cabe em 1 TB a 5 PB/mês | T1.4, T3.3 |
| **V4** exatidão | 100% de reconstrução bit-exata em ≥3 plataformas (x86 Intel, x86 AMD, aarch64) | matriz de testes + corpus dourado | **parcial**: 0 violações em 1 plataforma | T2.6, T4.5 |
| **V5** integridade | 10⁶ bit flips, truncamentos e perdas de bloco: detecção 100%, contenção ≤1 bloco, reparo por erasure | injeção de falhas | **não medida** (BLAKE3 sim; *scrubbing* e erasure não) | T4.1–T4.4 |
| **V6** latência | p99 de reconstrução de 1 MiB e de 1 GB dentro do SLA do piloto | motor com I/O real | **parcial**: descompressão 1,2–3,0 GB/s/núcleo, sem I/O | T2.2, T5.3 |

## 2. Decisões e dependências

| ID | Decisão / dependência | Quem | Padrão adotado até haver resposta | Estado |
|---|---|---|---|---|
| D1 | Cenário-alvo | Osney | **Resolvida (2026-11-04):** cliente com **5 PB/mês de processamento** que armazena "um pouco menos que isso" (assumo retenção de 90%, a trocar pelo número real); objetivo: reduzir o custo de **processamento e armazenamento**. Dois fluxos no motor (compressão e descompressão). Modelo em [pesquisa 9.7](pesquisa-compressao-neural.md) | **resolvida** |
| D2 | Ambiente de build e teste do motor | — | **Docker** (imagem `rust:1.88-slim-bookworm`, a do `Dockerfile`), código montado somente leitura, sem instalar Rust no host | **resolvida** (T1.1) |
| D3 | Hardware de teste | Osney | **Decisão (2026-11-04): avaliar a capacidade da máquina local e ajustar os testes a ela.** Servidor deixa de ser bloqueio; o V1 definitivo passa a depender da calibração local (T1.11) e fica **com a ressalva de ser notebook híbrido** | **resolvida; execução em T1.11** |
| D4 | Dataset real para validar CDC/delta/classificador/OpenZL e definir SLA | Osney | **O Osney disponibilizará o dataset quando o Claude avisar que tudo está pronto** (critérios na Seção 10). Até lá: dados públicos e sintéticos | **aguardando o aviso do Claude** |
| D5 | Camada neural fria (F2–F4) | Osney | **congelada** até o piloto mostrar nicho de texto/código frio com break-even | assumida |
| D6 | Retirar a rede neural do caminho de dados | Osney | **Confirmado (2026-11-04): sim.** Rede neural só no plano de controle e numa camada fria opcional congelada (D5) | **resolvida** |
| D7 | Leitura do RAPL (`energy_uj`) para medir Wh/GB | Osney | **Resolvida (2026-11-04):** o Osney aplicou o comando corrigido (Seção 11); verificado pelo Claude: 4 contadores `-r--r--r--`, leitura de potência funcionando (package-0 ~58 W sob carga de build). **Vale até reiniciar a máquina** (o reinício restaura `-r--------`) | **resolvida** |
| D8 | Análise de liberdade de operação da patente US11990923 (seleção de compressão por modelo de custo financeiro, [pesquisa 10.5](pesquisa-compressao-neural.md)) | Osney (jurídico) | **Adiada a pedido do Osney (2026-11-04).** Até lá, não comercializar a seleção por custo; **não é parecer jurídico** | **adiada** |
| D9 | Posicionamento após a varredura (OpenZL como baseline/codec; diferenciar pela decisão econômica do cliente, integridade verificável, on-prem + API, cobrança por economia) | Osney | **Adiada a pedido do Osney (2026-11-04):** foco em execução e estruturação do produto | **adiada** |

## 3. Frentes de trabalho

| Frente | Escopo | Fases |
|---|---|---|
| **W1** Medição e validação | benchmark v2, F1 em dados reais, microbenchmarks em Rust, servidor, energia | 1 |
| **W2** Motor: caminho síncrono | envelope v4 em blocos, fluxo (streaming), probe de incompressibilidade, paralelismo por bloco | 2 |
| **W3** Motor: planejamento | planejador por bloco, recompressão assíncrona por M\*, referência entre versões, dicionários, `/analyze` econômico | 3 |
| **W4** Integridade e operação | *scrubbing*, erasure, verificação em outro nó, injeção de falhas, determinismo entre plataformas, métricas | 4 |
| **W5** Produto e mercado | varredura de concorrentes, posicionamento, preço, piloto | 1 e 5 |
| **W6** Faixa neural (congelada) | F2 (baseline na RTX 5060), F3 (determinismo), F4 (falhas) | só após gate |

## 4. Tarefas por fase

Estados: **feito**, **em execução**, **pendente**, **bloqueado** (por D#).

### Fase 1: fundação e medição (semanas 1–2)

| ID | Tarefa | Entrega e critério de aceite | Depende | Estado |
|---|---|---|---|---|
| T1.1 | Baseline do motor em contêiner | `cargo test` verde. **122 testes passam, 0 falham; build limpo (35 s)**. Comando na Seção 8 | D2 | **feito** |
| T1.2 | Classificador de plano por bloco (experimento em Python) | [bench/plano_bloco.py](../bench/plano_bloco.py), [resultados](../bench/resultados/plano_bloco.md), [pesquisa 10.9](pesquisa-compressao-neural.md). **Veredito: não vale agora.** `zstd-3` fixo já está a 1,1–1,6% do oráculo; nenhum classificador cumpre ganho ≥1% sobre o *probe* e custo ≤2% (o melhor, CART a 1 MiB, passa só com o custo nativo **estimado**). O valor está em dados numéricos (`científico`: 11–13% com transform por bloco) | — | **feito** |
| T1.3 | Varredura de concorrentes (F0 do roadmap) | veredito por componente da hipótese de diferencial, com fontes. **Resultado ([pesquisa 10.5](pesquisa-compressao-neural.md)):** nenhum produto com as 4 peças, mas OpenZL cobre (2), SCOPe e a patente US11990923 cobrem (1), FinOps e Granica cobrem (4). Hipótese parcial | — | **feito** |
| T1.4 | F1 em dados menos favoráveis ao arquivo inteiro: imagens ext4 construídas com `mke2fs -d` a partir das árvores do kernel (tipo VM), pares de dumps de banco, fluxos de backup; medir CDC, `--long` e delta | ganho extra sobre arquivo inteiro + zstd-3 e índice, com números; **abandonar CDC** se <5% também aqui | D4 (ideal) | pendente |
| T1.5 | Microbenchmarks em Rust (no contêiner): BLAKE3 vs blake2b, `zstd` crate vs CLI, chunk+hash+lookup | MB/s por núcleo; confirma ~0,9 GB/s de chunk+hash+lookup ou melhor | D2 | pendente |
| T1.6 | Memória e custo de `zstd --long=31` e de `--patch-from` em escala | RSS de compressão e de descompressão; limite de janela por tenant | — | pendente |
| T1.7 | Escala sustentada e fator de calibração na máquina local (substitui o servidor, D3) | ver T1.11 | D3 | **substituída por T1.11** |
| T1.8 | Energia (J/GB) por configuração | RAPL lido durante os testes sustentados, com tratamento de *wrap* do contador (`max_energy_range_uj`) | D7 | **desbloqueada**; roda junto com T1.11 na janela exclusiva |
| T1.9 | **Avaliar o Meta OpenZL** como baseline e candidato a codec: build no contêiner; medir nos corpora do benchmark (classes reais, kernel) com e sem perfil treinado contra zstd-1/3/19 e xz-9e; razão, MB/s por núcleo, bit-exatidão; o que o *trainer* exige de dados; licença BSD; formato do frame (versão, checksums, digest do grafo) | **decisão: adotar, reservar ou descartar**, com números | D9 | pendente (logo após T1.11) |
| T1.10 | **Modelo do cliente** (processamento + armazenamento): parametrizar em `bench/economia.py` retenção, custo por GB movido, passagens, leituras e preço por camada; lista de perguntas sobre o pipeline do cliente | sensibilidade reproduzível; perguntas ao cliente | D1 | **modelo inicial feito** ([9.7](pesquisa-compressao-neural.md), [modelo_cliente.py](anexos/modelo_cliente.py)); parametrização no `economia.py` pendente |
| T1.11 | **Capacidade da máquina local e ajuste dos testes** ([capacidade-local.md](capacidade-local.md)): inventário, I/O do NVMe, largura de banda de memória, vazão **sustentada** (≥10 min, com frequência e temperatura) em 8 P-cores, 16 e 32 threads, calibração de 1 núcleo contra a referência pública, e quanto de 5 PB/mês a máquina sustenta | relatório com limites e fatores de correção; testes futuros usam esse perfil | D3 | **interrompida a pedido do Osney em 2026-11-04**; `bench/capacidade_local.py` e `bench/membw.c` foram criados e **não foram validados**; `resultados/capacidade_local.json` é parcial. Medido antes: inventário e I/O do NVMe (leitura 3,4 GB/s, escrita 2,1 GB/s, fila 1). Retomar só quando o Osney pedir |
| T1.12 | **Ferramenta de perfil de dataset** (`bench/perfil_dataset.py`): somente leitura, saída anonimizada, sem rede; inventário, classificação, dedup, famílias de versões (delta), compressibilidade por classe com IC e estimativa econômica com os parâmetros do cliente (cobre o T1.10) | `autoteste` ≥25 testes; erro da amostra vs medido inteiro; nenhuma vazão de nome na saída; registro do que foi lido para o procedimento de manuseio | D1 | **em execução** |

### Fase 2: caminho síncrono no motor (semanas 3–6)

| ID | Tarefa | Entrega e critério de aceite | Depende | Estado |
|---|---|---|---|---|
| T2.1 | **Envelope v4 em blocos independentes** (1 MiB; 64 KiB opcional): tabela de blocos (offset, plano, tamanhos, hash), digest do dicionário; leitura compatível com v≤3. Desenho em [ADR 001](adr-001-envelope-v4-blocos.md) | aceites A1–A9 do ADR: round-trip exaustivo, corpus dourado v2/v3, fuzz do parser; **0 regressões** nos 122 testes | T1.1 | **desenho feito**; implementação pendente (começa pelo corpus dourado) |
| T2.2 | **Pipeline em fluxo** (sem `Bytes` inteiro em RAM) para compressão e descompressão, com backpressure | arquivo de 5 GiB com pico de RSS limitado a N blocos em voo (alvo ≤512 MiB, a calibrar); latência p99 por bloco medida | T2.1 | pendente |
| T2.3 | *Probe* de incompressibilidade por bloco → `stored` | ≥5 GB/s por núcleo em dado incompressível; **0% de expansão** | T2.1 | pendente |
| T2.4 | Paralelismo por bloco | zstd-3 em blocos de 1 MiB ≥ 287 MB/s/núcleo (η = 0,42) e curva de η medida no servidor | T2.2, D3 | pendente |
| T2.5 | Verificação: BLAKE3 por bloco e do arquivo; *verify-on-write* opcional em thread/nó distinto | divergência vira erro, nunca dado adulterado; teste de corrupção injetada | T2.1 | pendente |
| T2.6 | Matriz de testes: round-trip, propriedades (`proptest`), fuzz, corpus dourado; plataformas x86 Intel (local), x86 AMD e aarch64 (nuvem ou emulação) | **V4**: 100% bit-exato nas três | T2.1 | pendente |

### Fase 3: planejamento e camadas (semanas 5–9)

| ID | Tarefa | Entrega e critério de aceite | Depende | Estado |
|---|---|---|---|---|
| T3.1 | **Planejador por bloco simples** (revisado após T1.2): *probe* de incompressibilidade → `stored`; `zstd-3` (ou `zstd-1` por vazão); **seleção de transform (delta/byte_split) por amostra só em blocos numéricos**; plano no envelope. **Sem modelo aprendido** | ≥ `zstd-3` fixo em bytes; ganho ≥9% em dado numérico; custo ≤2% do tempo; classes incompressíveis ficam `stored`. Reabrir o classificador só se features nativas medirem ≤2% e o IC do ganho ficar >1% (dados do cliente) | T2.1 (T1.2 resolvido) | pendente |
| T3.2 | **Recompressão assíncrona guiada por M\***: fila com prioridade por ganho/custo, CPU espalhada pelo mês, tabela M\* por tenant/preço/camada | simulação com carga sintética reproduz os M\* do benchmark (≈3,7 meses em S3, ≈21,5 em Glacier IR); nunca reempacota quando M\* = "nunca" | T2.1 | pendente |
| T3.3 | **Referência entre versões**: detecção de família (caminho/prefixo/tamanho, MinHash), `patch-from` e janela longa; limite de memória por tenant | ≥20% sobre arquivo inteiro + zstd-3 em par minor do kernel (limite superior medido: 29%); memória medida | T1.4, T1.6 | pendente |
| T3.4 | **Dicionários** por classe/tenant com versionamento e digest no envelope | ≥5% em texto de 64 KiB (benchmark: +7,7% em zstd-3); falta de dicionário gera erro claro | T2.1 | pendente |
| T3.5 | **`/analyze` econômico**: valor líquido por GB e camada recomendada com os preços e a retenção do tenant | resposta reproduz `bench/economia.py` para os mesmos parâmetros | T3.1 | pendente |

### Fase 4: integridade e operação (semanas 6–10)

| ID | Tarefa | Entrega e critério de aceite | Depende | Estado |
|---|---|---|---|---|
| T4.1 | *Scrubbing* periódico do vault | detecta 100% das corrupções injetadas; reporta e agenda reparo | T2.5 | pendente |
| T4.2 | Redundância por erasure (RS ou LRC) sobre o vault | sobrevive à perda de k blocos; overhead medido (alvo 1,2–1,5x) | T4.1 | pendente |
| T4.3 | Verificação em **nó diferente** do que codificou | detecta SDC simulado de CPU | T2.5 | pendente |
| T4.4 | **Injeção de falhas (F4)**: 10⁶ *bit flips*, truncamentos, perda de bloco, perda de dicionário, restauro de dicionário a partir de backup | detecção 100%, contenção ≤1 bloco, restauro comprovado | T4.1–T4.3 | pendente |
| T4.5 | Determinismo entre plataformas do formato clássico (corpus dourado em x86 Intel/AMD e aarch64) | mesmos bytes comprimidos e reconstruídos nas três | T2.6 | pendente |
| T4.6 | Métricas e alertas (Prometheus): bytes por camada, backlog de recompressão, M\* efetivo, falhas de *verify* | painel + alerta em `syntra_verify_failures_total > 0` (já existe) | T3.2 | pendente |

### Fase 5: piloto (semanas 10–16)

| ID | Tarefa | Entrega e critério de aceite | Depende | Estado |
|---|---|---|---|---|
| T5.1 | Escolher vertical e cliente-piloto | contrato de escopo, dados e SLA de latência | D4, T1.3 | pendente |
| T5.2 | Piloto de 50–100 TB, on-premise | operação contínua por ≥4 semanas | Fases 2–4 | pendente |
| T5.3 | Medição: US$/GB líquido real, Wh/GB, p50/p99 de reconstrução, η em produção | relatório de aceite V1–V6 | T5.2, D7 | pendente |
| T5.4 | Decisão sobre a faixa neural | só reabrir F2–F4 se o piloto mostrar texto/código frio com break-even | T5.3 | pendente |

### Faixa neural (congelada)

| ID | Tarefa | Gate |
|---|---|---|
| T6.1 | **F2**: ts_zip/NNCP/L3TC na RTX 5060 contra xz e zstd-19, com modelo contado | ΔS ≥ 3 p.p. e vazão ≥ 0,5 MB/s (nuvem) ou ≥ 20 MB/s (on-prem), senão parar |
| T6.2 | **F3**: determinismo cross-hardware (≥4 plataformas, ≥10⁹ símbolos) | qualquer falha no caminho float elimina o float |
| T6.3 | **F4**: injeção de falhas no formato neural | detecção 100% e contenção ≤1 segmento |

## 5. Riscos e mitigação

| # | Risco | Mitigação | Sinal de alerta |
|---|---|---|---|
| R1 | η pior em servidor real (acima de 16 núcleos não há dado) | T1.7 cedo; manter o dimensionamento por faixa (η 0,42 a 0,85) | η < 0,5 a 16 núcleos homogêneos |
| R2 | Dados reais não mostram ganho de CDC/delta como o kernel sugere | T1.4 e D4; o dedup de arquivo inteiro já existe como base | extra <5% em VM/banco/backup |
| R3 | Concorrente já oferece a composição (por exemplo, compressão por formato treinada) | T1.3 **feito**: materializou-se em parte (OpenZL, SCOPe, patente US11990923, Granica); reposicionar (D9), OpenZL como baseline (T1.9), parecer jurídico (D8) | veredito de T1.3 |
| R4 | O classificador de plano não vale | T1.2 decide objetivamente; probe simples como alternativa | `probe+zstd-3` a ≤1% do oráculo |
| R5 | Envelope v4 quebra compatibilidade ou aumenta a complexidade | leitura v≤3 mantida e testada; migração só por recompressão assíncrona | qualquer regressão nos testes legados |
| R6 | Memória da janela longa (2 GiB) inviabiliza `--long` por tenant | T1.6; janela adaptativa por classe | RSS acima do orçamento do nó |
| R7 | Estimativas de prazo otimistas | recalibrar ao fim da Fase 1; cortar escopo pelos gates | atraso >30% na Fase 1 |
| R8 | Quota do `/tmp` (tmpfs com `usrquota`, ~7,5 GiB reais) trava experimentos | extrair uma versão por vez e ler tar por offset | `EDQUOT` |

## 6. Critérios de parada globais

1. **Parar o CDC** se T1.4 também der <5% de ganho extra sobre arquivo inteiro + zstd-3 e delta/janela longa cobrirem os casos.
2. **Parar o classificador** (T1.2/T3.1) se o probe simples ficar a ≤1% do oráculo.
3. **Reposicionar** o produto se T1.3 mostrar que a composição (1)+(2) já existe no mercado. **Acionado em parte em 2026-11-04** ([pesquisa 10.5](pesquisa-compressao-neural.md)).
4. **Não reabrir a faixa neural** sem piloto positivo (T5.4).
5. **Reavaliar a premissa de preço** a cada semestre: NAND/DRAM subiram ~3,6x em 2026; é o único eixo que favorece o neural.

## 7. Cadência e responsabilidades

- **Osney decide e fornece:** D1, D3, D4, D6, D7, cliente-piloto e SLA.
- **Claude e agentes executam:** medições, protótipos, código do motor, testes e relatórios, com resultado conferido por amostragem e **sempre com o que não foi verificado dito**.
- **Revisão a cada fim de fase:** atualizar este plano, a pesquisa e o cofre (`cortex`); nada é commitado sem pedido.
- **Convenções:** sem comentários no código (o "porquê" vai ao README e a `docs/`); testes do motor só em contêiner; downloads em diretórios novos e vazios, tratados como não confiáveis.

## 8. Comando de teste do motor (T1.1)

```
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm cargo test --locked
```

`--cpuset-cpus=16-31` restringe o build aos E-cores deste notebook para não interferir em medições nos P-cores; ajuste ou remova em outra máquina.

## 9. Execução iniciada em 2026-11-04

| Item | Estado | Observação |
|---|---|---|
| T1.1 | **feito** | 122/122 testes; contêiner em E-cores |
| T1.2 | **feito** | veredito "não vale agora" (pesquisa 10.9); T3.1 revisado para planejador simples |
| T1.3 | **feito** | varredura de mercado: hipótese parcial (D8, D9, T1.9) |
| T2.1 (desenho) | **feito** | [ADR 001](adr-001-envelope-v4-blocos.md): envelope v4 em quadro com rodapé, blocos de 1 MiB, fluxo; achados do código (payload único, checksum de 64 bits, ≥3x o arquivo em RAM) |
| Próximos | T1.4, T1.5, T1.6 | após T1.2 liberar os P-cores; T1.7, T1.8 aguardam D3 e D7 |

## 10. Quando avisar o Osney para disponibilizar o dataset (D4)

O Claude avisará quando **todos** os itens abaixo estiverem prontos. Até lá, nenhum dado do cliente é necessário.

| # | Item | Verificação |
|---|---|---|
| 1 | Perfilamento de um diretório arbitrário e classificação automática (extensão, *magic bytes*, entropia), com relatório por classe, somente leitura e anonimizado | `bench/perfil_dataset.py` (T1.12, em execução) |
| 2 | Modelo econômico parametrizável com preços, retenção, leituras e custo de movimento do cliente (T1.10) | estimativa econômica de `bench/perfil_dataset.py` (T1.12), conferida contra `docs/anexos/modelo_cliente.py` |
| 3 | Capacidade da máquina local calibrada (T1.11) | [capacidade-local.md](capacidade-local.md) |
| 4 | OpenZL avaliado e incluído no harness (T1.9) | decisão adotar/reservar/descartar |
| 5 | Pipeline de dedup/delta (F1) pronto para versões arbitrárias | `bench/dedup.py` parametrizado por pares |
| 6 | Classificador de plano com veredito (T1.2) | critério de parada aplicado |
| 7 | **Procedimento de manuseio do dataset por escrito:** dados só na máquina local, leitura somente, nada enviado a serviço externo nem à web, subagentes sem acesso à web nessa fase, diretório isolado, registro do que foi lido, remoção ao final | aprovado pelo Osney antes do primeiro byte |
| 8 | Folga de armazenamento para o dataset | NVMe com ~287 GB livres; o `/tmp` (tmpfs) tem ~7,5 GiB reais |

**O que perguntar ao Osney sobre o dataset** (para decidir tamanho e escopo da amostra): (a) volume total e distribuição por tipo; amostra representativa sugerida de 50–200 GB (limite prático de disco local); (b) formatos (Parquet/CSV, logs, imagens, binários, VM, backups); (c) se há **versões sucessivas** dos mesmos dados (necessárias para dedup/delta); (d) o que o cliente chama de "processamento", onde a compressão entraria e se o consumo aceita dado comprimido; (e) volume lido por mês e padrão quente/frio; (f) preços atuais de armazenamento, movimento e computação, e retenção; (g) restrições de confidencialidade (dados pessoais, LGPD) e se podem ficar na máquina; (h) SLA de latência de leitura; (i) se já está comprimido ou criptografado.

## 11. Comando para liberar a leitura do RAPL (D7)

O Claude não consegue aplicar (o `sudo` exige senha). No terminal do Osney (**comando corrigido em 2026-11-04**: a primeira versão usava `/sys/class/powercap`, cujas entradas são links simbólicos que o `find` não segue, e não tinha efeito; o caminho real é `/sys/devices/virtual/powercap`, que contém 4 arquivos `energy_uj`):

```
sudo find /sys/devices/virtual/powercap -name energy_uj -exec chmod a+r {} +
```

Efeito temporário: some ao reiniciar. Para reverter antes disso:

```
sudo find /sys/devices/virtual/powercap -name energy_uj -exec chmod 400 {} +
```

Aviso (de memória, confira): contadores de energia legíveis por usuários comuns são uma fonte conhecida de canal lateral (família Platypus, CVE-2020-8694), por isso vêm restritos ao root; reverter após as medições.

## 12. Foco de execução (2026-11-04): estruturar e tornar o motor utilizável

Por decisão do Osney, D8 e D9 ficam adiadas; o foco é **executar o plano, estruturar o projeto e entregar o máximo de demanda utilizável**. Marcos de entrega do motor (cada um verde em `cargo test` no contêiner antes de seguir; ver Seção 8):

| Marco | Entrega | Tarefas | Aceite | Estado |
|---|---|---|---|---|
| **M1** | Formato v4 em blocos como módulo isolado, **corpus dourado v2/v3**, *probe* de incompressibilidade e testes (round-trip, corrupção, fuzz determinístico); especificação em `docs/` | T2.1 (parte), T2.3, T2.5 (parte), T2.6 (parte) | A1–A5 e A7 do [ADR 001](adr-001-envelope-v4-blocos.md); **0 regressões** nos 122 testes | **feito e verificado** (2026-11-04): `cargo test --locked` com **174 passam, 0 falham, 2 ignorados** (122 antigos + 3 corpus dourado + 41 formato v4 + 8 probe); `git diff` vazio em `container`, `planner`, `service`, `codec`, `transform`, `Cargo.*`; `proto` só com acréscimos; 0 comentários. Fuzz de 24.977 entradas sem pânico; 10⁴ *bit flips* detectados 100% com o índice certo; probe a 1,02% do `zstd-3` em E-core (não medido em P-core). Detalhes: [blockfile-v4.md](blockfile-v4.md) |
| **M2** | **Utilizabilidade:** teste de fumaça ponta a ponta em Docker, cliente de linha de comando, índice de documentação e README alinhado ao que foi medido | (novo) | `scripts/smoke.sh` termina com PASS; o cliente compacta e restaura um arquivo contra o serviço | **feito e verificado** (2026-11-04): `scripts/smoke.sh` com 81 PASS e 0 FAIL, rodado pelo agente (SQLite e Postgres) e por mim (SQLite, 8 s, sem sobras no Docker) |
| **M3** | Integração do v4 ao serviço e à API (atrás de flag), fluxo (*streaming*) com janela limitada, `Range` e erro `block_corrupted` | T2.2, T2.4, T2.5 | A6 e A8 do ADR; v≤3 intacto | pendente (depende de M1) |
| **M4** | Planejador por bloco simples (*probe* + `zstd-3` + transform em blocos numéricos), `/analyze` econômico e modo de uso offline (`engine pack/unpack`) | T3.1, T3.5 | ≥ `zstd-3` fixo em bytes; ≥9% em dado numérico | pendente |
| **M5** | Recompressão assíncrona guiada por M\*, *scrubbing* e injeção de falhas | T3.2, T4.1, T4.4 | simulação reproduz os M\* do benchmark; detecção 100% | pendente |
| **M2b** | **Correções de defeitos do motor achados no smoke** (após M1, que ocupa `src/`): **B1** reingestão do `watch_in` após reinício (`src/watcher.rs:39`, registro `seen` só em memória; infla referências e consumo faturável); **B2** `413` devolvido em texto puro e não no corpo JSON de erro (`src/api/mod.rs:89`, `src/api/compress.rs:25`); **B3** eventos de `analyze`/`decompress`/`read` gravam `bytes_in=0` e `cpu_ms=0` (`src/service.rs:635-650`, `src/api/compress.rs:144,163`; confirmado por mim); **B4** `./watch_in` criado como `root:root` e porta 3002 publicada em todas as interfaces (`docker-compose.yml:44,47`, `docker-compose.prod.yml:51`); **B6 (segurança, achado do M1)** `transform::csv_invert` com `p + len > data.len()` sem soma verificada: um varint hostil num envelope enviado a `POST /decompress` causa pânico e, com `panic = "abort"`, **derruba o processo** (`src/transform.rs`, ~linha 381; vale também para v≤3); **B7 (segurança, achado do M1)** bomba de descompressão em brotli, xz, bzip2 e deflate: `codec::decode` só limita a saída em 64 GiB e o lz4 aloca pelo prefixo declarado (`src/codec.rs`, `decode`) | T4.6 (parte), robustez | cada defeito com teste que falha antes e passa depois; os 122 testes e o smoke seguem verdes | pendente |
| **B5** (limite, não defeito) | Pico de RSS ~3,5x o arquivo na compressão (302 MB → 1.054 MB; medição única), resolvido pelo fluxo do M3 | T2.2 | A6 do ADR | no M3 |

**Retomada (2026-11-04, a pedido do Osney):** T1.9 (OpenZL) em **fase A**, que não mede tempo (build em contêiner, funcionalidade, bit-exatidão e razão de compressão), enquanto M1 e M2 rodam; as medições de **tempo** (fase B do T1.9) e a **calibração sustentada com energia** (T1.11 + T1.8) exigem a máquina **exclusiva** (sem builds Rust em Docker, Chrome/VS Code ociosos, ~2 h) e entram numa janela própria depois de M1 e M2. T1.8 depende de o RAPL estar legível: o primeiro comando passado ao Osney **não tinha efeito** e foi corrigido (Seção 11). Faixa neural (D5) segue congelada; D8 e D9 seguem adiadas.
