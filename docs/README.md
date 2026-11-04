# Índice de docs/ e bench/

Escrito em 2026-11-04 (marco M2 do [plano de trabalho](plano-de-trabalho.md)). O estado de cada item é o que o próprio documento declara nessa data; quando um documento novo entrar em `docs/` (por exemplo, a especificação do formato v4 do marco M1), acrescente a linha aqui.

Para **usar** o motor, comece por [uso.md](uso.md) e pelo [README da raiz](../README.md). Os documentos abaixo explicam **por que** o projeto é como é e **para onde** vai.

## docs/

| Documento | O que é | Estado |
|---|---|---|
| [uso.md](uso.md) | Guia de uso e operação: subir (Docker, compose), variáveis, chaves e tenants, CLI, ingestão por diretório, backup, métricas e limites conhecidos | **Novo (M2).** Verificado em 2026-11-04 com `scripts/smoke.sh`; o que não foi executado está marcado |
| [plano-de-trabalho.md](plano-de-trabalho.md) | Plano para levar o motor a 5 PB/mês: condições V1 a V6, decisões D1 a D9, tarefas por fase, riscos e os marcos M1 a M5 (seção 12) | **Vivo.** Fase 1: T1.1 a T1.3 feitas, T1.4 a T1.6 pendentes; M1 e M2 em execução; M3 a M5 pendentes; faixa neural congelada |
| [pesquisa-compressao-neural.md](pesquisa-compressao-neural.md) | Pesquisa: estado da arte (neural e clássico), armazenamento, modos de falha, viabilidade de 5 PB/mês (seção 9) e benchmark com direção de desenvolvimento (seção 10, rodadas 1 a 3) | **Documento vivo, com 3 rodadas de benchmark registradas**; as seções 9 e 10 sustentam o plano. Contém números medidos e inferências, cada um rotulado |
| [adr-001-envelope-v4-blocos.md](adr-001-envelope-v4-blocos.md) | ADR 001: envelope v4 em blocos independentes de 1 MiB, rodapé com tabela de blocos e processamento em fluxo; critérios de aceite A1 a A9 | **Proposta, não ligada à API.** O marco M1 implementa o módulo isolado; a integração é o M3 |
| [anexos/](anexos/) | Cálculos de apoio da pesquisa (abaixo) | Scripts em Python, só biblioteca padrão, sem comentários; não foram reexecutados no M2 |

### docs/anexos/

| Arquivo | O que calcula |
|---|---|
| [modelo_cliente.py](anexos/modelo_cliente.py) | Modelo do cliente (processamento + armazenamento), seção 9.7 da pesquisa: economia de armazenamento, de movimento e custo de CPU por retenção |
| [breakeven.py](anexos/breakeven.py) e [cenarios_breakeven.json](anexos/cenarios_breakeven.json) | Margem por GB e volume mínimo para o ponto de equilíbrio, por cenário de preço e de vazão |
| [sensibilidade.py](anexos/sensibilidade.py) | Custo de processar 1 GB por vazão e vazão mínima para margem positiva, por tipo de armazenamento |
| [contas_derivadas.py](anexos/contas_derivadas.py) | Energia (processar contra economizar) e amortização de um modelo |

### Referências citadas pelo plano que não existem em docs/

- `capacidade-local.md` (tarefa T1.11): **não existe**. A tarefa foi interrompida a pedido do Osney; só há o resultado parcial `bench/resultados/capacidade_local.json`.

## bench/

Benchmark reprodutível de razão e vazão por núcleo, convertido em núcleos para 5 PB/mês e em US$/GB. Metodologia, comandos e limites: [bench/README.md](../bench/README.md). Mede **bibliotecas e CLIs de compressão em Python**, não o serviço HTTP em Rust.

| Arquivo | O que é |
|---|---|
| [README.md](../bench/README.md) | Como rodar, definição das métricas, limitações |
| [preparar_dados.py](../bench/preparar_dados.py) e [gerar_sinteticos.py](../bench/gerar_sinteticos.py) | Baixa Silesia e Canterbury, extrai com segurança, gera as classes sintéticas e o manifesto com sha256 |
| [syntrabench.py](../bench/syntrabench.py) | Coleta: razão, vazão, escala multinúcleo, memória, dicionário, P-core contra E-core |
| [economia.py](../bench/economia.py) e [precos.json](../bench/precos.json) | Modelo econômico (US$/GB líquido, núcleos, M\*) e os preços, cada um com fonte e tipo (verificado ou inferência própria) |
| [dedup.py](../bench/dedup.py) e [cdc.c](../bench/cdc.c) | Gate F1: deduplicação por blocos (tamanho fixo e FastCDC) contra arquivo inteiro |
| [escala_resumo.py](../bench/escala_resumo.py) | Resume a escala em núcleos homogêneos |
| [plano_bloco.py](../bench/plano_bloco.py) | Experimento T1.2: classificador de plano por bloco |
| [capacidade_local.py](../bench/capacidade_local.py) e [membw.c](../bench/membw.c) | T1.11 (capacidade da máquina): **interrompido e não validado** |

| Resultado | O que contém | Estado |
|---|---|---|
| [resumo.md](../bench/resultados/resumo.md) | Resumo do benchmark principal, gerado por `economia.py` (2026-11-03) | Rodada completa; **0 violações de bit-exatidão** |
| [medicoes.json](../bench/resultados/medicoes.json) e [economia.json](../bench/resultados/economia.json) | Dados brutos e modelo econômico | Idem |
| [dedup.md](../bench/resultados/dedup.md) e [dedup.json](../bench/resultados/dedup.json) | Gate F1 (rodada 2) | Veredito: CDC reprova em árvores de arquivos; em fluxos únicos ganha em alguns pares e reprova em outros |
| [escala_homogenea.md](../bench/resultados/escala_homogenea.md) e [.json](../bench/resultados/escala_homogenea.json) | Escala em núcleos homogêneos (rodada 2) | Até 16 núcleos; acima disso, sem dado |
| [plano_bloco.md](../bench/resultados/plano_bloco.md), [.json](../bench/resultados/plano_bloco.json) e [plano_bloco_rotulos.npz](../bench/resultados/plano_bloco_rotulos.npz) | Classificador de plano por bloco (rodada 3) | Veredito: não vale agora |
| [capacidade_local.json](../bench/resultados/capacidade_local.json) | Inventário e I/O da máquina local | **Parcial** |

`bench/openzl/` existe no diretório de trabalho mas **não está rastreado no git** e não é indexado aqui.

## scripts/

| Arquivo | O que faz |
|---|---|
| [smoke.sh](../scripts/smoke.sh) | Teste de fumaça ponta a ponta em Docker (constrói a imagem, sobe o serviço, comprime, restaura, mede, limpa tudo) |
| [syntra.sh](../scripts/syntra.sh) | Cliente de linha de comando sobre `curl` (comprimir, restaurar, baixar, analisar, inspecionar, listar, consumo) |

Os dois são descritos em [uso.md](uso.md#1-verificação-rápida). Convenção do projeto: nenhum comentário em código, nem em scripts; o "porquê" fica nos documentos.

## Raiz do projeto

| Arquivo | O que é |
|---|---|
| [README.md](../README.md) | Visão geral, garantias, API, configuração, medições, estado e roadmap |
| `Dockerfile`, `docker-compose.yml`, `docker-compose.prod.yml` | Imagem de produção, compose de desenvolvimento (Postgres, API aberta) e de produção (chaves e senha obrigatórias) |
| `Dockerfile.dev` | Imagem de desenvolvimento com `cargo watch` (não exercitada no M2) |
