# Syntra Engine

**Compressão sem perdas como API.** O motor recebe bytes, escolhe a estratégia
de compressão **medindo** candidatos reais sobre o próprio dado, e devolve um
container `.syntra` auto-suficiente que reconstrói o original bit a bit.

```
POST /api/v1/compress      bytes  ──▶  essência .syntra
POST /api/v1/decompress    .syntra ──▶  bytes originais
POST /api/v1/analyze       bytes  ──▶  comparativo de planos, sem gravar
```

Guia de operação: [docs/uso.md](docs/uso.md) · Segurança: [docs/seguranca.md](docs/seguranca.md) · Índice da documentação: [docs/README.md](docs/README.md) · Plano e marcos: [docs/plano-de-trabalho.md](docs/plano-de-trabalho.md)

---

## Índice

- [Garantias](#garantias)
- [Tenants, chaves e escopos](#tenants-chaves-e-escopos)
- [Medição de consumo e cobrança](#medição-de-consumo-e-cobrança)
- [Como o plano é escolhido](#como-o-plano-é-escolhido)
- [Resultados medidos](#resultados-medidos)
- [Desempenho medido](#desempenho-medido)
- [Início rápido](#início-rápido)
- [Verificação rápida](#verificação-rápida)
- [CLI](#cli)
- [Referência da API](#referência-da-api)
- [Modos de esforço](#modos-de-esforço)
- [Configuração](#configuração)
- [Armazenamento e operação](#armazenamento-e-operação)
- [Cobertura de algoritmos](#cobertura-de-algoritmos)
- [Desenvolvimento](#desenvolvimento)
- [Arquitetura do código](#arquitetura-do-código)
- [Estado e roadmap](#estado-e-roadmap)
- [Compatibilidade](#compatibilidade)

---

## Garantias

| Garantia | Como é sustentada |
|---|---|
| **Reconstrução bit a bit** | O BLAKE3 do original fica no envelope e é conferido em **toda** descompressão. Divergência devolve `422`, não dado adulterado. |
| **Verificação antes de gravar** | Com `SYNTRA_VERIFY_ON_WRITE=true` (default), o motor descomprime a essência e compara com o original *antes* de persistir. Se não bater, nada é gravado e o cliente recebe erro. |
| **A essência nunca é maior** | Todo conjunto de candidatos inclui um piso `stored`. Se nenhum codec reduzir, grava-se sem compressão. O *payload* nunca excede o original; o container adiciona de ~200 a ~430 bytes de metadados (nome, hash, plano, timestamps), de modo que um arquivo de poucos bytes gera um `.syntra` **maior** que ele (medido no `scripts/smoke.sh`: +200 B em arquivo vazio, +339 B em 1 byte, +411 B em um CSV de 5 MB). |
| **Auto-suficiência** | O envelope guarda a receita completa de reconstrução: pilha de transforms, codec, nível e id do dicionário. Reconstruir não depende de estado do processo. |
| **Dicionário nunca invalida o passado** | Dicionários treinados são endereçados por conteúdo (`<classe>.<digest>`). Retreinar gera um id novo; essências antigas continuam resolvendo o dicionário original. |
| **Deduplicação global** | O vault é endereçado pelo hash do conteúdo. Um blob por conteúdo único, compartilhado entre tenants; cada tenant vê apenas o que lhe pertence. |
| **Isolamento entre tenants** | O acesso passa por um *grant* `(tenant, objeto)`. Conhecer o id de um objeto de outro tenant devolve `404`. |
| **Cobrança sem confiar no cliente** | Todo valor faturável sai dos contadores que executam o trabalho. Nada vindo do corpo da requisição entra no cálculo. |
| **Envelope hostil não derruba o motor** | Toda reconstrução tem teto de saída em todos os codecs (o menor entre o tamanho declarado no envelope e `SYNTRA_MAX_DECOMPRESSED_MB`); estouro devolve `422 decompressed_too_large`. Aritmética de comprimentos verificada nos transforms; fuzz determinístico com mais de 20.000 casos por transform e por corpus. Modelo de ameaça e lacunas: [docs/seguranca.md](docs/seguranca.md). |

---

## Como o plano é escolhido

A pergunta "qual algoritmo usar?" não é respondida por heurística fixa — é
**medida**.

```
                     ┌──────────────────────────────────────────┐
   bytes ───────────▶ │ 1. CLASSIFICA                            │
                     │    MIME, entropia, razão de imprimíveis, │
                     │    periodicidade (stride), layout tabular│
                     └────────────────┬─────────────────────────┘
                                      ▼
                     ┌──────────────────────────────────────────┐
                     │ 2. GERA CANDIDATOS                       │
                     │    transform* + codec + nível + dict     │
                     │    (1, 4 ou 16 planos, conforme esforço) │
                     └────────────────┬─────────────────────────┘
                                      ▼
                     ┌──────────────────────────────────────────┐
                     │ 3. MEDE em paralelo (Rayon)              │
                     │    arquivo inteiro, ou amostra se grande │
                     └────────────────┬─────────────────────────┘
                                      ▼  menor saída vence
                     ┌──────────────────────────────────────────┐
                     │ 4. PISO `stored` + VERIFICA round-trip   │
                     └────────────────┬─────────────────────────┘
                                      ▼
                     ┌──────────────────────────────────────────┐
                     │ 5. SELA o envelope e grava no vault      │
                     └──────────────────────────────────────────┘
```

**Transforms** reescrevem os bytes para uma forma onde a redundância fica
adjacente, sem perder informação. É onde estão os ganhos que um codec genérico
sozinho não alcança:

| Transform | O que faz | Ganha em |
|---|---|---|
| `csv_columnar` | Transpõe texto tabular para layout coluna a coluna | CSV, TSV, NDJSON de campos uniformes |
| `delta` | Substitui cada elemento pela diferença do anterior | Timestamps, IDs sequenciais, contadores, PCM |
| `byte_split` | Separa os planos de bytes dos elementos (estilo Parquet) | Arrays de `f32`/`f64`, inteiros de faixa estreita |
| `rle` | Run-length encoding (PackBits) | Bitmaps, máscaras, regiões constantes |

Depois vem o **codec** de entropia/dicionário:

| Codec | Família | Níveis | Dicionário |
|---|---|---|---|
| `stored` | passthrough | — | — |
| `lz4` | LZ77 | — | — |
| `deflate` | LZ77 + Huffman | 0–9 | — |
| `zstd` | LZ77 + FSE | -7–22 | sim |
| `bzip2` | BWT + MTF + Huffman | 1–9 | — |
| `brotli` | LZ77 + Huffman + contexto | 0–11 | estático embutido |
| `xz` | LZMA2 | 0–9 | — |

`GET /api/v1/codecs` devolve esse catálogo em runtime — dá para montar chamadas
sem hardcodar nome nem faixa de nível.

---

## Resultados medidos

Medição real, mesmo binário, mesmo hardware. A coluna **ANTES** reproduz a
política da versão anterior do motor (LZ4 ou `zstd -5`, escolhidos por dois
limiares de entropia) forçando codec/nível pela própria API — comparação de
política contra política.

| Arquivo | Original | ANTES | AGORA (`max`) | Essência menor em | Plano vencedor |
|---|---:|---:|---:|---:|---|
| `vendas.csv` (400 mil linhas) | 29,3 MB | 55,4% | **83,6%** | −63,3% | `csv_columnar+xz:6` |
| `eventos.ndjson` (120 mil logs) | 20,1 MB | 67,8% | **88,7%** | −64,7% | `csv_columnar+bzip2:9` |
| `dump.sql` (150 mil INSERTs) | 16,5 MB | 77,6% | **91,9%** | −63,9% | `bzip2:9` |
| `serie.i64` (900 mil timestamps) | 7,2 MB | 37,4% | **88,0%** | −80,8% | `delta:8>byte_split:8+zstd:19` |
| `telemetria.f32` (1,5 M sensores) | 6,0 MB | −0,0% | **54,4%** | −54,4% | `byte_split:4+xz:6` |
| `aleatorio.bin` (incompressível) | 6,0 MB | −0,0% | 0,0% | 0,0% | `stored` |
| **Total** | **85,0 MB** | **53,3%** | **78,8%** | **−54,6%** | |

Todas as reconstruções conferidas por SHA-256 contra o original: idênticas.

> **Procedência desta tabela.** Ela vem de uma medição anterior do motor Rust
> (corpus sintético de 85,0 MB). Os arquivos de entrada e a saída bruta **não
> estão versionados** em `bench/`, e a tabela **não foi reproduzida** na
> verificação do marco M2. Os números **verificáveis** a partir de arquivos do
> repositório estão em [Desempenho medido](#desempenho-medido). O que o M2
> reexecutou é o ciclo funcional (`scripts/smoke.sh`), não esta medição.

Dois casos merecem destaque:

- **`telemetria.f32`**: a política anterior escolhia LZ4 e *expandia* o arquivo.
  Um array de `f32` não tem redundância que o LZ77 enxergue — ela está na
  correlação entre bytes na mesma posição de elementos vizinhos, que só aparece
  depois do `byte_split`.
- **`serie.i64`**: timestamps crescentes viram valores quase constantes após
  `delta:8`, e o `byte_split` agrupa os bytes altos (todos iguais). De 37,4%
  para 88,0%.

---

## Desempenho medido

Fonte única destes números: [`bench/resultados/resumo.md`](bench/resultados/resumo.md)
(gerado por `bench/economia.py` a partir de `medicoes.json`, 2026-11-03;
metodologia em [`bench/README.md`](bench/README.md)). Mix real = arquivos do
Silesia e do Canterbury; blocos independentes de 1 MiB; **1 núcleo**; zstd 1.5.7.

- **zstd -3**: razão **2,57** a **378 MB/s** na compressão e **1.611 MB/s** na
  descompressão, por núcleo (seções 4 e 5 do resumo).
- **zstd -1**: razão **2,35** a **596 MB/s** na compressão e **1.707 MB/s** na
  descompressão (seções 4 e 5).
- **Níveis altos custam CPU, não descompressão**: xz -9e razão **3,16** a **5 MB/s**
  por núcleo; zstd -19 razão **2,95** a **7 MB/s** (seções 4 e 5).
- **Escala do zstd -3** (processos independentes): **3.170 MB/s** com 8 núcleos
  físicos (eficiência 0,95) e **5.846 MB/s** com 32 threads (eficiência 0,42)
  (seção 3).
- **Dado incompressível**: zstd -3 percorre **5.539 MB/s** por núcleo e devolve
  razão **1,00**, o que sustenta o piso `stored` (seções 4 e 5).
- **Custo do bloco**: o tamanho comprimido em blocos de 1 MiB é **1,3%** maior que
  o do arquivo inteiro (zstd -3, mix real); em blocos de 64 KiB, **10,3%**
  (seção 6).
- **Bit-exatidão**: **0 violações** e **0 células com falha** em todas as medições
  (seções 1 e 15).

**O que esses números são e não são.** São medições de **bibliotecas e CLIs de
compressão** num **notebook híbrido** (Intel i9-14900HX: 8 núcleos de desempenho
e 16 de eficiência, governor `powersave`), com um *harness* em Python, dados em
tmpfs e fatia máxima de 8 MiB por arquivo. **Não há medição do servidor HTTP em
Rust sob carga**, nem em hardware de servidor, nem com I/O real de rede e disco;
acima de 16 núcleos homogêneos não há dado de escala. As razões do motor
(planejador com transforms) podem diferir das razões de um codec isolado.
Detalhes e limites: seção 10.3 de
[`docs/pesquisa-compressao-neural.md`](docs/pesquisa-compressao-neural.md).

---

## Início rápido

### Docker

```bash
# desenvolvimento (Postgres + engine, esforço max, API aberta)
mkdir -p watch_in          # sem isto o `up` falha (de propósito: evita um diretório de root)
docker compose up --build

# produção
export SYNTRA_API_KEYS="$(openssl rand -hex 32)"
export POSTGRES_PASSWORD="$(openssl rand -hex 24)"
docker compose -f docker-compose.prod.yml up -d --build
```

Dashboard: <http://localhost:3002/> · Contrato: <http://localhost:3002/api/v1/openapi.json>

Os dois compose publicam a porta `3002` só em **loopback** (`127.0.0.1`) por
padrão; para abrir a outra interface defina `SYNTRA_PUBLISH_HOST` (e
`SYNTRA_PUBLISH_PORT` para trocar a porta). No de desenvolvimento a API é
**aberta** (sem chave) e `./watch_in` precisa existir antes do `up` (o compose
recusa o diretório ausente em vez de deixar o Docker criá-lo como `root`). Sem
Postgres (nó único), use SQLite:

```bash
docker build -t syntra-engine .
docker run -d --name syntra -p 127.0.0.1:3002:3002 -v syntra-data:/data \
  -e SYNTRA_DB_URL=sqlite:/data/syntra.db \
  -e SYNTRA_API_KEYS="$(openssl rand -hex 32)" syntra-engine
```

A imagem aponta por padrão para um Postgres em `postgres:5432`; sem
`SYNTRA_DB_URL` o contêiner encerra com `conectando no PostgreSQL`. Passo a passo
completo, backup e limites em [docs/uso.md](docs/uso.md).

### Local

Não é preciso instalar `protoc`: o binário vem vendorizado pelo build script.

```bash
SYNTRA_DB_URL=sqlite:syntra.db cargo run --release
```

### Primeiro ciclo

```bash
# comprime e guarda o container
curl -sS -X POST 'http://localhost:3002/api/v1/compress?filename=vendas.csv&effort=max' \
     --data-binary @vendas.csv -D headers.txt -o vendas.syntra

grep -i x-syntra headers.txt
#   x-syntra-id: 9f2c...            ← hash BLAKE3, é o id do objeto
#   x-syntra-savings-pct: 83.61
#   x-syntra-plan: csv_columnar+xz:6
#   x-syntra-verified: true

# reconstrói e confirma que é idêntico
curl -sS -X POST http://localhost:3002/api/v1/decompress \
     --data-binary @vendas.syntra -o reconstruido.csv
cmp vendas.csv reconstruido.csv && echo "idêntico"
```

Os valores comentados acima são ilustrativos (do arquivo de 29,3 MB da tabela de
resultados). Com `SYNTRA_API_KEYS` definido, acrescente `-H "x-api-key: $CHAVE"` a
cada chamada. O mesmo ciclo pela [CLI](#cli): `scripts/syntra.sh compress vendas.csv`
e `scripts/syntra.sh decompress vendas.csv.syntra --out reconstruido.csv`.

---

## Verificação rápida

```bash
scripts/smoke.sh
```

Teste de fumaça ponta a ponta, em Docker, que prova que o ciclo de uso funciona
na sua máquina. Constrói a imagem do `Dockerfile` (`syntra-engine:smoke`), sobe um
contêiner **descartável** (SQLite, chave aleatória, volume próprio, porta alta só
em `127.0.0.1`) e confere, passo a passo, com `PASS` ou `FAIL`:

- comprimir, restaurar e comparar byte a byte (`cmp`) um CSV de 5 MB, um NDJSON,
  2 MB aleatórios, um arquivo vazio e 1 byte; reenvio deduplicado; dado aleatório
  vira `stored` sem expandir;
- `objects`, `analyze`, `codecs`, `health`, `ready`, `metrics`
  (`syntra_verify_failures_total 0`), OpenAPI e `stats`;
- `401` sem chave, `400` com id malformado, `404`, `422` com container adulterado;
- tenant e chave pelas rotas `admin`, `/usage`, idempotência, isolamento entre
  tenants, escopos e revogação;
- `analyze`, `decompress` e leitura gravando `bytes_in`, `bytes_out` e `cpu_ms`
  no ledger;
- um contêiner extra com `SYNTRA_MAX_BODY_MB=1`: `413` em JSON padrão e teto de
  descompressão;
- bombas reais de 1 GiB (lz4, deflate, xz, bzip2) e um envelope com varint
  colunar hostil: `422` rápido e o motor segue de pé;
- a porta publicada só em loopback (no contêiner de teste e nos dois compose);
- a [CLI](#cli) contra o próprio serviço, a ingestão por diretório (o arquivo
  inalterado **não** é reingerido depois do reinício), o reinício do contêiner
  (os dados persistem) e o encerramento por `SIGTERM`.

Ao final imprime a tabela `arquivo, tamanho, essência, economia, plano, ok`, remove
contêiner, volume e rede (inclusive se falhar ou for interrompido) e sai com
código diferente de zero em qualquer `FAIL`. `SYNTRA_SMOKE_DB=postgres` repete o
teste com um Postgres descartável. Pré-requisitos, variáveis e o tempo medido
estão em [docs/uso.md](docs/uso.md#1-verificação-rápida).

---

## CLI

`scripts/syntra.sh` é um cliente fino sobre `curl` (só `curl` e `python3`, sem
dependências).

```bash
export SYNTRA_URL=http://127.0.0.1:3002     # padrão
export SYNTRA_API_KEY=<chave>               # nunca é impressa

scripts/syntra.sh compress vendas.csv --effort max      # grava vendas.csv.syntra
scripts/syntra.sh decompress vendas.csv.syntra --out reconstruido.csv
scripts/syntra.sh analyze vendas.csv
scripts/syntra.sh list --search vendas
scripts/syntra.sh get <id> --out original.csv
scripts/syntra.sh usage
```

Comandos: `compress`, `decompress`, `get`, `analyze`, `inspect`, `list`, `stats`,
`usage`, `codecs`, `health`; `--help` lista as opções. Erros da API saem legíveis
(`error.code`, `message`, `hint`) com código de saída 1; uso incorreto sai com 2 e
falha de conexão com 3. Nunca sobrescreve arquivo sem `--force`. Detalhes em
[docs/uso.md](docs/uso.md#5-cli).

---

## Referência da API

Contrato completo em `GET /api/v1/openapi.json` (OpenAPI 3.0.3) — importável no
Postman/Insomnia e utilizável com `openapi-generator`.

### Compressão

| Método | Rota | O que faz |
|---|---|---|
| `POST` | `/api/v1/compress` | Bytes → essência. Devolve o container ou o relatório JSON. |
| `POST` | `/api/v1/decompress` | Essência → bytes originais, com hash conferido. |
| `POST` | `/api/v1/analyze` | Mede todos os candidatos e devolve o comparativo. Não grava. |
| `POST` | `/api/v1/inspect` | Lê metadados e valida o checksum de uma essência. Não reconstrói. |

### Objetos arquivados

| Método | Rota | O que faz |
|---|---|---|
| `GET` | `/api/v1/objects` | Lista os objetos do tenant (`?search=&limit=`). `search` casa um trecho do **nome do primeiro envio** daquele conteúdo; `limit` vai de 1 a 1000 (padrão 100). Não existe filtro por MIME nem por id. |
| `GET` | `/api/v1/objects/:id` | Metadados: plano, economia, referências. |
| `GET` | `/api/v1/objects/:id/content` | Original reconstruído. |
| `GET` | `/api/v1/objects/:id/essence` | Container `.syntra` cru (para replicar sem descomprimir). |
| `DELETE` | `/api/v1/objects/:id` | Solta uma referência; `?purge=true` remove já. |

`:id` é o hash BLAKE3 do original: 64 dígitos hexadecimais. Qualquer outra coisa
devolve `400 invalid_object_id`.

### Descoberta e operação

| Método | Rota | O que faz |
|---|---|---|
| `GET` | `/api/v1/codecs` | Codecs, transforms, modos de esforço e classes de conteúdo. |
| `GET` | `/api/v1/dictionaries` | Dicionários treinados e em uso. |
| `GET` | `/api/v1/stats` | `process` (esta instância) e `lifetime` (acumulado em banco). |
| `GET` | `/health` | Liveness. Não toca banco nem disco. |
| `GET` | `/ready` | Readiness: confere banco e vault. |
| `GET` | `/metrics` | Prometheus. |

`/health`, `/ready`, `/metrics` e `/api/v1/openapi.json` ficam abertos mesmo com
autenticação ativa.

### Parâmetros de compressão

Aceitos em query string **ou** header. Precedência: query > header > default.

| Query | Header | Valores | Default |
|---|---|---|---|
| `filename` | `x-filename` | texto | `sem-nome` |
| `mime` | `x-mime` | MIME | detectado |
| `effort` | `x-effort` | `fast` `balanced` `max` | `SYNTRA_DEFAULT_EFFORT` |
| `codec` | `x-codec` | nome do codec | medição automática |
| `level` | `x-level` | inteiro | default do codec |
| `verify` | `x-verify` | bool | `SYNTRA_VERIFY_ON_WRITE` |
| `dictionary` | `x-dictionary` | bool | automático |
| `persist` | — | bool | `true` |
| `dedup` | — | bool | `true` |
| `response` | `x-response-format` | `binary` `json` | `binary` |

`Accept: application/json` equivale a `response=json`.

Nível fora da faixa do codec devolve `400 invalid_level` — sem clamp silencioso.

### Corpo de erro

```json
{
  "error": {
    "code": "dictionary_unavailable",
    "message": "dicionário 'application_json.7f3a1c…' necessário para reconstruir não está disponível neste nó",
    "hint": "Restaure o diretório SYNTRA_DICT_PATH ou aponte SYNTRA_REDIS_URL para o cluster que o contém."
  }
}
```

`code` é estável e pode ser tratado programaticamente. Os valores estão
enumerados no OpenAPI.

| Status | Quando |
|---|---|
| `400` | Parâmetro inválido, id malformado, corpo vazio, envelope indecifrável |
| `401` | API key ausente ou inválida |
| `404` | Objeto inexistente |
| `409` | Dicionário necessário indisponível neste nó |
| `422` | Essência corrompida, infiel ao original ou irreconstruível; `decompressed_too_large` quando a reconstrução passaria do teto (o menor entre o tamanho declarado no envelope e `SYNTRA_MAX_DECOMPRESSED_MB`) |
| `413` | Corpo acima de `SYNTRA_MAX_BODY_MB`, em **todas** as rotas com corpo; o formato é o JSON acima (`payload_too_large`, com `hint`) |
| `503` | Motor encerrando (requisições não são rejeitadas por fila: elas aguardam o permit de CPU) |

### Autenticação

```bash
curl -H 'x-api-key: syn_a1b2c3d4e5f6_<segredo>' ...
curl -H 'Authorization: Bearer syn_a1b2c3d4e5f6_<segredo>' ...
```

A chave determina o **tenant** e os **escopos**. Ver
[Tenants, chaves e escopos](#tenants-chaves-e-escopos).

A API exige credencial quando existe qualquer chave emitida **ou**
`SYNTRA_API_KEYS` está definido. Sem nenhum dos dois, a API sobe aberta
operando no tenant `default` e registra um aviso no log.

Atenção ao dashboard embutido: com autenticação ativa, as rotas de dados que ele
consome (`/stats`, `/api/files`) também passam a exigir chave, e a página fica
sem dados. Em produção, sirva o dashboard atrás de um proxy que injete o header,
ou trate-o como ferramenta de desenvolvimento e monitore por `/metrics`.

### Idempotência

Envie `Idempotency-Key: <seu-id>` no `POST /api/v1/compress`. Repetir a mesma
chave no mesmo tenant devolve o resultado anterior com `replayed: true`, sem
reprocessar e **sem gerar novo evento de cobrança** — é o que torna seguro o
retry automático de um cliente HTTP.

### Correlação

`x-request-id` é propagado se enviado, ou gerado, e volta na resposta.

---

## Tenants, chaves e escopos

O modelo tem quatro peças. A separação importa: o blob é global (é dele que vem
a economia de storage), mas a **autorização é por tenant**.

```
tenant
  ├── api_key     (n)   escopos, expiração, revogação; só o hash é guardado
  ├── grant       (n)   (tenant × objeto) → referências        ← fronteira de acesso
  └── usage_event (n)   append-only, um por requisição         ← base da fatura
objeto (conteúdo único, global)
  └── essência          blob endereçado por conteúdo
```

### Por que grant e não posse no objeto

Como o id do objeto é o hash do conteúdo, dois tenants que enviam os mesmos
bytes chegam ao mesmo id. Sem uma fronteira explícita isso vazaria de duas
formas: um tenant leria o objeto do outro sabendo o id, e o campo
`deduplicated` viraria um oráculo revelando que *alguém* já armazenou aquele
arquivo.

O grant fecha as duas. E a assinatura das funções de acesso exige o
`tenant_id`, então a checagem não é algo que o autor de um handler precise
lembrar de fazer — sem ele o código não compila.

Consequência visível na API: `deduplicated: true` só aparece quando o **próprio
tenant** já tinha aquele conteúdo. Reaproveitamento entre tenants acontece no
disco e é registrado internamente (`dedup_cross_tenant` no consumo), mas nunca
é revelado ao cliente.

### Escopos

| Escopo | Libera |
|---|---|
| `compress` | `POST /api/v1/compress`, `/process`, `/api/sim/process` |
| `read` | `decompress`, `analyze`, `inspect`, `GET /objects*`, `/usage`, `/stats`, `/codecs` |
| `delete` | `DELETE /api/v1/objects/:id` |
| `admin` | `/api/v1/admin/*` (implica todos os outros) |

O escopo é exigido por **grupo de rotas**, não dentro do handler. Chave válida
sem o escopo recebe `403 forbidden`.

### Ciclo de vida

```bash
# 1. bootstrap: SYNTRA_API_KEYS opera no tenant `default` com todos os escopos
export ADMIN="$SYNTRA_API_KEYS"

# 2. cria o tenant do cliente
curl -sS -X POST http://localhost:3002/api/v1/admin/tenants \
  -H "x-api-key: $ADMIN" -H 'content-type: application/json' \
  -d '{"id":"acme","name":"ACME S.A.","plan":"savings_share",
       "price":{"share_pct":30,"reference_gb_month_cents":12}}'

# 3. emite a chave — o segredo aparece SÓ nesta resposta
curl -sS -X POST http://localhost:3002/api/v1/admin/tenants/acme/keys \
  -H "x-api-key: $ADMIN" -H 'content-type: application/json' \
  -d '{"name":"producao","scopes":["compress","read"],"expires_in_days":365}'
# → {"key":"syn_9f2c4a1b8d3e_7c…","warning":"A chave é exibida apenas nesta resposta…"}

# 4. revoga (efeito imediato, conferido a cada requisição)
curl -sS -X DELETE http://localhost:3002/api/v1/admin/keys/9f2c4a1b8d3e \
  -H "x-api-key: $ADMIN"
```

O servidor guarda apenas o BLAKE3 do segredo. Não há rota que o devolva, e
`GET .../keys` lista somente metadados.

---

## Medição de consumo e cobrança

Cada requisição que persiste grava **uma** linha em `usage_events` — tabela
append-only. Remover um objeto não apaga o histórico de consumo.

| Dimensão | O que registra |
|---|---|
| `bytes_in` | bytes ingeridos |
| `bytes_out` | tamanho da essência (compressão) ou bytes servidos (leitura) |
| `bytes_saved` | `original − essência` — a **métrica de valor** |
| `effort` | tier de esforço, porque `max` custa ~260× a CPU do `fast` |
| `cpu_ms` | custo real de processamento |
| `dedup_scope` | `none`, `tenant` ou `global` |
| `operation` | `compress`, `decompress`, `read`, `analyze`, `delete` |

Todas as dimensões são gravadas sempre, independentemente do plano. Trocar de
plano recalcula faturas antigas sem reprocessar nada.

Todas as operações preenchem as três dimensões (corrigido no M2b; no M2 os
eventos de `decompress`, `read` e `analyze` saíam com `bytes_in` e `cpu_ms`
zerados). `decompress`: `bytes_in` é o tamanho do container recebido e
`bytes_out` o original reconstruído; `read` de `/content`: `bytes_in` é o payload
gravado e `bytes_out` o original; `read` de `/essence`: os dois são o tamanho do
container; `analyze`: `bytes_in` é o corpo recebido e `bytes_out` o JSON
devolvido; `cpu_ms` é o tempo medido de cada operação. Como esses eventos agora
têm `bytes_in`, `totals.bytes_in` do `/usage` soma **todas** as operações
(`by_operation` traz a divisão), enquanto `totals.savings_pct`, a cotação e o
`by_effort` usam só os eventos de `compress`; a fatura dos dois planos não muda.

### As duas fórmulas

O plano do tenant escolhe qual se aplica:

**`savings_share`** — percentual da economia entregue.

```
valor = (bytes_saved em GiB) × reference_gb_month_cents × share_pct / 100
```

Autofinanciado: o cliente só paga porque a fatura de storage caiu. É o modelo
mais forte para a vertical de custo de nuvem, porque remove a objeção de compra.

**`ingested_gb`** — por volume, com tier de esforço.

```
valor = Σ por tier: (bytes_in do tier em GiB) × per_gb_cents[tier]
```

Previsível para quem prefere orçamento fixo. Exige o tier porque o esforço `max`
consome ordens de magnitude mais CPU.

Leitura e reconstrução entram no ledger mas **não** entram na fatura destes dois
planos — ficam disponíveis para um plano com egress no futuro.

### Precisão do valor

O valor sai em `amount_micro_cents` (10⁻⁶ centavo) e não em centavos. Não é
detalhe: um arquivo de 40 MiB economizado rende 0,139 centavo, que arredondado
por linha vira **zero**. Num produto medido, centenas dessas requisições por dia
somariam receita real e apareceriam como nada. Acumule e feche a competência
sobre `amount_micro_cents`; `amount_cents` existe só para exibição.

### Consultando

```bash
curl -sS 'http://localhost:3002/api/v1/usage?days=30&events=20' \
  -H "x-api-key: $CHAVE" | jq '{totals: .totals, quote: .quote}'

scripts/syntra.sh usage --days 30 --events 20      # sem jq
```

```json
{
  "totals": {
    "events": 1284, "bytes_in": 91234567890, "bytes_saved": 71234567890,
    "savings_pct": 78.08, "dedup_same_tenant": 141, "dedup_cross_tenant": 12,
    "by_effort": [{ "effort": "max", "events": 1284, "bytes_in": 91234567890 }]
  },
  "quote": {
    "plan": "savings_share",
    "formula": "30% de 12 centavos por GiB-mês de armazenamento evitado",
    "currency": "BRL",
    "amount_micro_cents": 238824000,
    "amount_cents": 239,
    "lines": [{ "label": "armazenamento economizado", "quantity_gib": 66.34,
                "unit_cents_per_gib": 3.6, "amount_micro_cents": 238824000 }]
  }
}
```

`GET /api/v1/admin/usage?tenant_id=acme` dá a mesma visão para qualquer tenant.

---

## Modos de esforço

`effort` controla quantos planos o motor mede:

| Modo | Candidatos | Perfil |
|---|---|---|
| `fast` | 1 | Plano heurístico único. Latência mínima. |
| `balanced` | até 4 | Default. Bom ganho com custo previsível. |
| `max` | até 16 | Varredura ampla de transforms × codecs. Densidade máxima. |

Medido sobre o mesmo corpus de 85,0 MB da seção anterior (tempo inclui a
verificação de round-trip, que está ligada por padrão):

| Modo | Essência total | Economia | Tempo |
|---|---:|---:|---:|
| Política anterior | 39,7 MB | 53,3% | — |
| `fast` | 24,7 MB | **70,9%** | 0,5 s |
| `balanced` | 20,3 MB | **76,1%** | 11,3 s |
| `max` | 18,0 MB | **78,8%** | 130,4 s |

O detalhe que importa: `fast` gasta meio segundo e ainda assim comprime muito
mais que a política anterior. Ela não era um ponto de trade-off — era pior nas
duas dimensões, porque escolhia LZ4 e níveis negativos de Zstd, que ficam fora
da curva útil de densidade/velocidade.

Para arquivos acima de `SYNTRA_PROBE_THRESHOLD_MB` (48 MB por padrão), a
medição roda sobre uma amostra e só o plano vencedor é aplicado ao arquivo
inteiro — o custo de CPU fica limitado sem abrir mão de decidir com dado real.
A resposta JSON marca isso em `sampled_decision`.

Use `POST /api/v1/analyze` para ver o trade-off antes de escolher:

```bash
curl -sS -X POST 'http://localhost:3002/api/v1/analyze?filename=vendas.csv&effort=max' \
     --data-binary @vendas.csv | jq '.candidates[:4]'
```

---

## Configuração

| Variável | Default | Descrição |
|---|---|---|
| `SYNTRA_BIND_ADDR` | `0.0.0.0:3002` | Endereço de escuta |
| `SYNTRA_MAX_BODY_MB` | `4096` | Corpo máximo aceito (o compose de produção usa `10240`). O corpo é lido **inteiro em memória** (pico medido de ~3,5x o arquivo na compressão): veja [docs/uso.md](docs/uso.md#9-limites-conhecidos). Acima do limite: `413 payload_too_large` em JSON |
| `SYNTRA_MAX_DECOMPRESSED_MB` | igual a `SYNTRA_MAX_BODY_MB` | Teto de saída de toda reconstrução; vale o menor entre ele e o `original_size` do envelope. Acima: `422 decompressed_too_large`. Veja [docs/seguranca.md](docs/seguranca.md#3-limites-configuráveis) |
| `SYNTRA_API_KEYS` | — | Chaves estáticas de bootstrap; operam no tenant `default` com todos os escopos |
| `SYNTRA_CORS_ORIGINS` | — | Origens permitidas (vazio = liberado) |
| `SYNTRA_VAULT_PATH` | `essence_vault` | Raiz das essências |
| `SYNTRA_SLED_INDEX` | `metadata_index.sled` | Índice de referências |
| `SYNTRA_DICT_PATH` | `dictionaries` | Dicionários treinados |
| `SYNTRA_DB_URL` | `postgres://…` | `postgres://…` ou `sqlite:arquivo.db` |
| `SYNTRA_DEFAULT_EFFORT` | `balanced` | `fast` · `balanced` · `max` |
| `SYNTRA_VERIFY_ON_WRITE` | `true` | Confere round-trip antes de gravar |
| `SYNTRA_DICTIONARIES` | `true` | Habilita treino e uso de dicionários |
| `SYNTRA_REDIS_URL` | — | Distribui dicionários entre nós |
| `SYNTRA_PROBE_THRESHOLD_MB` | `48` | Acima disto, mede por amostra |
| `SYNTRA_PROBE_SAMPLE_MB` | `8` | Tamanho da amostra de medição |
| `SYNTRA_KEEP_OUTPUT_MB` | `16` | Até onde reaproveitar a saída do vencedor |
| `SYNTRA_WATCH_ENABLED` | `true` | Ingestão por diretório |
| `SYNTRA_WATCH_DIR` | `watch_in` | Diretório observado |
| `SYNTRA_WATCH_DELETE_SOURCE` | `false` | Remove o original após arquivar |
| `SYNTRA_COMPRESS_MULTIPLIER` | `4` | Permits de I/O por permit de CPU |
| `RAYON_NUM_THREADS` | CPUs | Threads de compressão |
| `RUST_LOG` | `engine=info` | Filtro de log |

Valor inválido gera aviso e cai no default — um typo não impede o serviço de
subir.

Variáveis lidas só pelos arquivos compose (não pelo motor):

| Variável | Default | Descrição |
|---|---|---|
| `SYNTRA_PUBLISH_HOST` | `127.0.0.1` | Interface em que a porta do motor é publicada (`0.0.0.0` para todas) |
| `SYNTRA_PUBLISH_PORT` | `3002` | Porta publicada no host |
| `SYNTRA_POSTGRES_PORT` | `5432` | Porta do Postgres do compose de desenvolvimento, sempre em `127.0.0.1` |
| `SYNTRA_WATCH_HOST_DIR` | `./watch_in` | Diretório do host montado em `/data/watch_in` (compose de desenvolvimento; precisa existir) |
| `SYNTRA_CONTAINER_PREFIX` | `syntra` | Prefixo dos nomes de contêiner, e no de produção também dos volumes e da rede, para subir duas instâncias lado a lado |

---

## Armazenamento e operação

### Layout

```
essence_vault/
  9f/2c/9f2c4a…e1.syntra        # uma essência por conteúdo único
metadata_index.sled/            # hash → {referências, tamanhos, nomes vistos}
dictionaries/
  text_csv.7f3a1c2b8d4e5f60.dict
  application_json.a1b2c3d4e5f60718.dict
```

O caminho é derivado do hash; **nenhuma string vinda do cliente entra no
caminho de arquivo**. O identificador recebido pela API é decodificado para 32
bytes antes de virar caminho.

### O que precisa de backup

Três diretórios e o banco, juntos:

1. `SYNTRA_VAULT_PATH` — as essências.
2. `SYNTRA_DICT_PATH` — **indispensável**. Uma essência comprimida com
   dicionário só reconstrói com aquele dicionário. Sem ele, a descompressão
   devolve `409 dictionary_unavailable`. Em cluster, aponte `SYNTRA_REDIS_URL`
   para que os dicionários sejam replicados.
3. `SYNTRA_SLED_INDEX` — reconstruível a partir do vault no boot (verificado no
   M2: apagado o diretório e reiniciado, o log registra `índice do vault
   reconciliado ... reindexadas=1` e o objeto segue legível).
4. **O banco** — aqui isto mudou de status. Ele deixou de ser só auditoria:
   `tenants`, `api_keys`, `object_grants` e `usage_events` vivem nele. Perder o
   banco significa perder quem pode acessar o quê e o histórico de consumo que
   sustenta a fatura. As essências continuam íntegras e reconstruíveis, mas o
   produto para de funcionar. Trate o banco como dado crítico. Verificado no M2
   (SQLite apagado e motor reiniciado): `GET /objects` lista 0, `/objects/:id/content`
   devolve `404`, mas `POST /decompress` com o `.syntra` guardado reconstrói o
   arquivo, e reenviar o original recria o acesso. Chaves e tenants não voltam.

### Ingestão por diretório

Arquivos colocados em `SYNTRA_WATCH_DIR` são arquivados automaticamente pela
mesma camada de serviço da API. O motor espera o tamanho estabilizar antes de
ler (não arquiva cópia em andamento) e só remove o original se
`SYNTRA_WATCH_DELETE_SOURCE=true` **e** o arquivamento tiver confirmado sucesso.

Regras (lidas em `src/watcher.rs`; o arquivamento foi verificado no
`scripts/smoke.sh`): o diretório **não é recursivo**; arquivos vazios, nomes que
começam com `.` ou `~` e extensões `.tmp`, `.part`, `.crdownload`, `.swp` e
`.syntra` são ignorados; tudo é arquivado no tenant `default`, com o esforço de
`SYNTRA_DEFAULT_EFFORT`; cada arquivo é lido inteiro em memória. A essência vai
para o vault, não de volta ao diretório: recupere o original por
`GET /api/v1/objects` e `/content`.

**Reinício não reingere (corrigido no M2b).** O registro do que já foi visto fica
no banco (tabela `watch_seen`: caminho, tamanho, mtime em ns e hash do objeto;
`src/watch_registry.rs`) e é carregado no boot. Arquivo inalterado não é lido de
novo; se só o mtime mudou, o motor relê o arquivo, confere o BLAKE3 e, sendo igual,
só atualiza o registro (nenhuma referência nem evento novo); conteúdo diferente vira
objeto novo (a versão anterior mantém a sua referência); arquivo que sumiu sai do
registro. No M2 isso valia só dentro do processo: depois de um reinício `references`
foi de 1 para 2 e o ledger ganhou um segundo evento `compress`. Hoje o smoke
confere 1 referência e nenhum evento novo depois de `docker restart`. Limites:
[docs/seguranca.md](docs/seguranca.md#6-o-que-ainda-não-está-coberto).

### Encerramento

O binário trata `SIGTERM`/`SIGINT`, drena as conexões em curso e sincroniza o
índice antes de sair. Em produção, dê `stop_grace_period` folgado.

### Métricas

```
syntra_bytes_in_total / syntra_essence_bytes_total / syntra_distillation_ratio
syntra_items_total / syntra_reconstructions_total / syntra_errors_total
syntra_verify_failures_total        # deve ser sempre 0 — alerte se subir
syntra_dedup_hits_total / syntra_dedup_bytes_saved_total
syntra_codec_usage_total{codec="…"}
syntra_processing_latency_ms_bucket{le="…"}
syntra_uptime_seconds / syntra_system_cpu_usage / syntra_system_memory_used_bytes
```

`syntra_verify_failures_total > 0` significa que uma essência não reproduziu o
original na verificação de gravação — bug de codec ou memória com defeito. O
motor recusa gravar nesse caso, mas é um sinal para investigar imediatamente.

---

## Cobertura de algoritmos

O motor cobre as famílias que se aplicam a **arquivo entra, arquivo sai, sem
perdas**:

| Família | Implementado |
|---|---|
| Entropy coding | Huffman, FSE, range coding (internos aos codecs) |
| Dictionary / Lempel-Ziv | LZ4, DEFLATE, Zstd, Brotli, LZMA2 |
| BWT | Bzip2 |
| Predictive coding | `delta` (largura 1/2/4/8) |
| Columnar | `csv_columnar`, `byte_split` (Byte Stream Split do Parquet) |
| Run/pattern | `rle` (PackBits) |
| Dicionário treinado | Zstd dictionary, treinado por classe de conteúdo |
| Deduplicação | Global, endereçada por conteúdo (BLAKE3) |

**Deliberadamente fora de escopo**, com o motivo:

- **Codecs perceptuais** (JPEG, AV1, HEVC, MP3, Opus) e **quantização** são
  *lossy*. O contrato do motor é reconstrução bit a bit; um modo com perdas
  seria um contrato diferente, com endpoint e garantias próprias. Conteúdo que
  já usa esses codecs é detectado como `media` e gravado `stored` em vez de
  recomprimido em vão.
- **Recompressão de container** (Zopfli em PNG, recompressão de JPEG, reempacotar
  ZIP/XLSX) exige parsear cada formato. É a próxima fronteira útil de densidade,
  mas é trabalho por formato, não algoritmo genérico.
- **Compressão de modelos e de vetores** (quantização INT8/INT4, GPTQ, PQ/OPQ,
  pruning, distillation) opera sobre tensores com semântica conhecida e tolera
  perda controlada. Não é a mesma operação que "reduzir um arquivo e reconstruí-lo".
- **Chunking com deduplicação em nível de bloco** (CDC/Rabin) está modelado no
  protobuf (`Block`) mas não ativado: rende em coleções de arquivos com grandes
  trechos comuns, um caso que o dedup por arquivo inteiro já cobre parcialmente.

---

## Desenvolvimento

```bash
cargo test              # roundtrip, classificação, integridade, robustez, API ponta a ponta
cargo build --release
cargo clippy --all-targets
```

Os testes não exigem serviços externos: o banco roda em `sqlite::memory:` e os
testes de API sobem o router completo em processo.

Cobertura relevante:

- **Roundtrip exaustivo** — todo codec e todo transform, em todas as larguras,
  incluindo entrada vazia, de 1 byte e de tamanho não múltiplo do elemento.
- **Bijetividade dos transforms** — `invert(apply(x)) == x`, com validação de
  tamanho em cada estágio.
- **Piso `stored`** — dado aleatório não gera essência maior que o original.
- **Integridade** — bit flip no payload é detectado antes de descomprimir; hash
  divergente vira erro de fidelidade.
- **Dicionários** — treino, persistência, resolução por id após restart, recusa
  de dicionário corrompido, e retreino que não invalida o anterior.
- **API ponta a ponta** — ciclo comprimir/reconstruir, dedup, autenticação,
  rejeição de id malicioso, forma dos erros, rotas legadas.
- **Isolamento entre tenants** — um tenant não lê, lista nem remove objeto de
  outro mesmo conhecendo o id; o blob só sai do disco na última referência
  global; `deduplicated` não vaza a existência de conteúdo alheio.
- **Escopos e chaves** — escopo exigido por rota, revogação com efeito imediato,
  segredo nunca recuperável, expiração respeitada.
- **Cobrança** — as duas fórmulas sobre o mesmo ledger, idempotência que não
  cobra duas vezes, leitura registrada sem entrar na fatura, e `bytes_in`,
  `bytes_out` e `cpu_ms` preenchidos em `analyze`, `decompress` e leitura.
- **Robustez diante de entrada hostil** — fuzz determinístico (mais de 20.000
  casos por transform e por mutação do corpus dourado), bombas reais de 1 GiB em
  todos os codecs, envelopes forjados e `413`/`422` em JSON. Detalhes em
  [docs/testes.md](docs/testes.md).
- **Ingestão por diretório** — o registro persistido evita reingestão depois de
  reiniciar o motor.

Para medir com dados próprios, use `POST /api/v1/analyze` — ele devolve o
tamanho e o tempo de cada candidato sem gravar nada.

---

## Arquitetura do código

| Arquivo | Responsabilidade |
|---|---|
| `src/codec.rs` | Registro de codecs, faixas de nível, encode/decode |
| `src/transform.rs` | Transformações reversíveis (delta, byte-split, RLE, colunar) |
| `src/content.rs` | Classificação: MIME, entropia, stride, layout tabular |
| `src/planner.rs` | Geração e medição de candidatos; seleção do vencedor |
| `src/container.rs` | Envelope `.syntra`: selar, ler, verificar fidelidade |
| `src/dict_store.rs` | Dicionários treinados, persistidos, endereçados por conteúdo |
| `src/vault.rs` | Armazenamento endereçado por conteúdo e contagem de referências |
| `src/tenancy.rs` | Tenants, chaves, escopos, grants e configuração de preço |
| `src/usage.rs` | Ledger append-only de consumo e as duas fórmulas de cobrança |
| `src/service.rs` | Orquestração compartilhada por HTTP e ingestão |
| `src/sql.rs` | Macros que servem SQLite e Postgres sem duplicar cada query |
| `src/api/` | Router, middlewares, DTOs, erros, OpenAPI, rotas legadas |
| `src/watcher.rs` | Ingestão automática por diretório, com registro persistido |
| `src/watch_registry.rs` | Tabela `watch_seen` (o que a ingestão já arquivou) |
| `src/blockfile.rs`, `src/probe.rs` | Formato v4 em blocos e *probe* de incompressibilidade, isolados (M1), fora da API |
| `src/metrics.rs` | Contadores Prometheus e JSON |
| `src/db_monitor.rs` | Auditoria em SQLite/Postgres |
| `proto/engine.proto` | Formato do envelope |

Dependências: apenas o que é usado. Codecs (`zstd`, `brotli`, `xz2`, `bzip2`,
`flate2`, `lz4_flex`), HTTP (`axum`, `tower-http`), persistência (`sled`,
`sqlx`), paralelismo (`rayon`, `dashmap`) e o essencial de suporte. O build não
exige `protoc` no sistema nem pacotes `apt` na imagem.

Compressão é CPU-bound e I/O de vault é bloqueante: ambos rodam em
`spawn_blocking` sobre o pool do Rayon, nunca nas threads do runtime async. O
semáforo de CPU é adquirido apenas em volta do trabalho de CPU, não durante a
leitura do corpo HTTP.

O código-fonte não leva comentários, por convenção do projeto. O "porquê" das
decisões — política de compressão, garantias de integridade, trade-offs
operacionais — mora neste README. Ao alterar comportamento, atualize a seção
correspondente aqui. A convenção vale para os scripts: `scripts/*.sh` e
`bench/*.py` também não têm comentários.

---

## Estado e roadmap

**Hoje (2026-11-04, fechamento do M2b):** motor v0.3.0, envelope `version 3`,
`cargo test` com **224 testes verdes** (2 ignorados) e `cargo clippy
--all-targets` sem avisos, ciclo de uso verificado de ponta a ponta por
`scripts/smoke.sh` (SQLite e Postgres) e operável pela [CLI](#cli). Os limites que
importam para quem vai operar (arquivo inteiro em RAM, limite de corpo, teto de
descompressão, envelope único) estão em [docs/uso.md](docs/uso.md#9-limites-conhecidos);
o que ainda **não** está coberto em segurança, em
[docs/seguranca.md](docs/seguranca.md#6-o-que-ainda-não-está-coberto).

**Para onde vai:** levar o motor a uma linha de vazão de **5 PB/mês**, com
planejamento por bloco, recompressão assíncrona, referência entre versões e
integridade provada; a rede neural fica fora do caminho de dados. O plano
completo (condições V1 a V6, decisões, tarefas, riscos) está em
[docs/plano-de-trabalho.md](docs/plano-de-trabalho.md); a evidência, em
[docs/pesquisa-compressao-neural.md](docs/pesquisa-compressao-neural.md) e
[bench/](bench/README.md). Marcos do motor, copiados do plano (a fonte é o plano):

| Marco | Entrega | Estado no plano |
|---|---|---|
| M1 | Formato v4 em blocos como módulo isolado, corpus dourado v2/v3, *probe* de incompressibilidade ([ADR 001](docs/adr-001-envelope-v4-blocos.md)) | feito |
| M2 | Utilizabilidade: teste de fumaça, CLI, índice de documentação, README alinhado ao que foi medido | feito |
| M2b | Correção dos defeitos achados no smoke e no M1: ingestão que reingeria a cada reinício (B1), `413` em JSON (B2), ledger de leitura (B3), compose em loopback e `./watch_in` sem root (B4), pânico em `csv_invert` (B6), bomba de descompressão (B7) | feito |
| M3 | Integração do v4 à API atrás de flag, fluxo (*streaming*) com janela limitada, `Range`, erro `block_corrupted` | pendente (depende do M1) |
| M4 | Planejador por bloco simples, `/analyze` econômico, uso offline (`engine pack/unpack`) | pendente |
| M5 | Recompressão assíncrona guiada por M\*, *scrubbing*, injeção de falhas | pendente |

Fora do escopo por ora: faixa neural (congelada até um piloto mostrar nicho com
ponto de equilíbrio), energia por GB e avaliação do OpenZL. Duas decisões estão
adiadas a pedido do Osney (análise de liberdade de operação de uma patente sobre
seleção de compressão por custo financeiro e posicionamento de mercado); até lá,
não comercialize a seleção por custo. Isso **não é parecer jurídico**.

---

## Compatibilidade

### Rotas antigas

`/process`, `/compress`, `/reconstruct`, `/decompress`, `/stats`, `/api/files`,
`/api/files/:id`, `/api/sim/process` e `/api/sim/download/:id` continuam
respondendo no **mesmo formato**, mas por dentro passam pela camada de serviço
atual — herdando deduplicação, verificação, dicionários persistidos e seleção
por medição. Novas integrações devem usar `/api/v1`.

### Envelopes antigos

Essências gravadas com `version <= 2` (enum `Algorithm` legado, sem transforms)
continuam sendo lidas e reconstruídas. Envelopes novos são `version 3`.
