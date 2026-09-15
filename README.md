# Syntra Engine

**Compressão sem perdas como API.** O motor recebe bytes, escolhe a estratégia
de compressão **medindo** candidatos reais sobre o próprio dado, e devolve um
container `.syntra` auto-suficiente que reconstrói o original bit a bit.

```
POST /api/v1/compress      bytes  ──▶  essência .syntra
POST /api/v1/decompress    .syntra ──▶  bytes originais
POST /api/v1/analyze       bytes  ──▶  comparativo de planos, sem gravar
```

---

## Índice

- [Garantias](#garantias)
- [Tenants, chaves e escopos](#tenants-chaves-e-escopos)
- [Medição de consumo e cobrança](#medição-de-consumo-e-cobrança)
- [Como o plano é escolhido](#como-o-plano-é-escolhido)
- [Resultados medidos](#resultados-medidos)
- [Início rápido](#início-rápido)
- [Referência da API](#referência-da-api)
- [Modos de esforço](#modos-de-esforço)
- [Configuração](#configuração)
- [Armazenamento e operação](#armazenamento-e-operação)
- [Cobertura de algoritmos](#cobertura-de-algoritmos)
- [Desenvolvimento](#desenvolvimento)
- [Arquitetura do código](#arquitetura-do-código)

---

## Garantias

| Garantia | Como é sustentada |
|---|---|
| **Reconstrução bit a bit** | O BLAKE3 do original fica no envelope e é conferido em **toda** descompressão. Divergência devolve `422`, não dado adulterado. |
| **Verificação antes de gravar** | Com `SYNTRA_VERIFY_ON_WRITE=true` (default), o motor descomprime a essência e compara com o original *antes* de persistir. Se não bater, nada é gravado e o cliente recebe erro. |
| **A essência nunca é maior** | Todo conjunto de candidatos inclui um piso `stored`. Se nenhum codec reduzir, grava-se sem compressão. O *payload* nunca excede o original; o container adiciona ~300–400 bytes de metadados (nome, hash, plano, timestamps). |
| **Auto-suficiência** | O envelope guarda a receita completa de reconstrução: pilha de transforms, codec, nível e id do dicionário. Reconstruir não depende de estado do processo. |
| **Dicionário nunca invalida o passado** | Dicionários treinados são endereçados por conteúdo (`<classe>.<digest>`). Retreinar gera um id novo; essências antigas continuam resolvendo o dicionário original. |
| **Deduplicação global** | O vault é endereçado pelo hash do conteúdo. Um blob por conteúdo único, compartilhado entre tenants; cada tenant vê apenas o que lhe pertence. |
| **Isolamento entre tenants** | O acesso passa por um *grant* `(tenant, objeto)`. Conhecer o id de um objeto de outro tenant devolve `404`. |
| **Cobrança sem confiar no cliente** | Todo valor faturável sai dos contadores que executam o trabalho. Nada vindo do corpo da requisição entra no cálculo. |

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

Dois casos merecem destaque:

- **`telemetria.f32`**: a política anterior escolhia LZ4 e *expandia* o arquivo.
  Um array de `f32` não tem redundância que o LZ77 enxergue — ela está na
  correlação entre bytes na mesma posição de elementos vizinhos, que só aparece
  depois do `byte_split`.
- **`serie.i64`**: timestamps crescentes viram valores quase constantes após
  `delta:8`, e o `byte_split` agrupa os bytes altos (todos iguais). De 37,4%
  para 88,0%.

---

## Início rápido

### Docker

```bash
# desenvolvimento (Postgres + engine, esforço max, API aberta)
docker compose up --build

# produção
export SYNTRA_API_KEYS="$(openssl rand -hex 32)"
export POSTGRES_PASSWORD="$(openssl rand -hex 24)"
docker compose -f docker-compose.prod.yml up -d --build
```

Dashboard: <http://localhost:3002/> · Contrato: <http://localhost:3002/api/v1/openapi.json>

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
| `GET` | `/api/v1/objects` | Lista processamentos (`?type=name\|mime\|id&search=&limit=`). |
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
| `422` | Essência corrompida, infiel ao original ou irreconstruível |
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
| `SYNTRA_MAX_BODY_MB` | `4096` | Corpo máximo aceito |
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

Três diretórios, juntos:

1. `SYNTRA_VAULT_PATH` — as essências.
2. `SYNTRA_DICT_PATH` — **indispensável**. Uma essência comprimida com
   dicionário só reconstrói com aquele dicionário. Sem ele, a descompressão
   devolve `409 dictionary_unavailable`. Em cluster, aponte `SYNTRA_REDIS_URL`
   para que os dicionários sejam replicados.
3. `SYNTRA_SLED_INDEX` — reconstruível a partir do vault no boot.
4. **O banco** — aqui isto mudou de status. Ele deixou de ser só auditoria:
   `tenants`, `api_keys`, `object_grants` e `usage_events` vivem nele. Perder o
   banco significa perder quem pode acessar o quê e o histórico de consumo que
   sustenta a fatura. As essências continuam íntegras e reconstruíveis, mas o
   produto para de funcionar. Trate o banco como dado crítico.

### Ingestão por diretório

Arquivos colocados em `SYNTRA_WATCH_DIR` são arquivados automaticamente pela
mesma camada de serviço da API. O motor espera o tamanho estabilizar antes de
ler (não arquiva cópia em andamento) e só remove o original se
`SYNTRA_WATCH_DELETE_SOURCE=true` **e** o arquivamento tiver confirmado sucesso.

### Encerramento

O binário trata `SIGTERM`/`SIGINT`, drena as conexões em curso e sincroniza o
índice antes de sair. Em produção, dê `stop_grace_period` folgado.

### Métricas

```
syntra_bytes_in_total / syntra_essence_bytes_total / syntra_distillation_ratio
syntra_items_total / syntra_reconstructions_total
syntra_verify_failures_total        # deve ser sempre 0 — alerte se subir
syntra_dedup_hits_total / syntra_dedup_bytes_saved_total
syntra_codec_usage_total{codec="…"}
syntra_processing_latency_ms_bucket{le="…"}
syntra_system_cpu_usage / syntra_system_memory_used_bytes
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
cargo test              # roundtrip, classificação, integridade, API ponta a ponta
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
  cobra duas vezes, leitura registrada sem entrar na fatura.

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
| `src/watcher.rs` | Ingestão automática por diretório |
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
correspondente aqui.

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
