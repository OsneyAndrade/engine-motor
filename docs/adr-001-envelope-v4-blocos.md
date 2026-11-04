# ADR 001 (proposta): envelope v4 em blocos independentes e processamento em fluxo

Estado: **proposta, não implementada** · Data: 2026-11-04 · Tarefas do plano: T2.1, T2.2, T2.3, T2.5 ([plano de trabalho](plano-de-trabalho.md)) · Evidência: [pesquisa](pesquisa-compressao-neural.md) Seções 9 e 10 e [bench/](../bench/).

## 1. Contexto: o que o código faz hoje

Verificado em [src/container.rs](../src/container.rs), [src/service.rs](../src/service.rs), [src/api/compress.rs](../src/api/compress.rs) e [proto/engine.proto](../proto/engine.proto):

1. **Um envelope, um payload.** O arquivo comprimido inteiro vai no campo `data` (campo 10) de uma única mensagem protobuf `ProcessedFile` ([container.rs:131-176](../src/container.rs#L131-L176)); o `proto` tem `Block`, mas não é usado no caminho atual (`blocks: Vec::new()`).
2. **Integridade grossa.** `envelope_checksum` é **64 bits do BLAKE3 do payload inteiro** ([container.rs:191-194](../src/container.rs#L191-L194)), conferido antes de descomprimir ([container.rs:212-227](../src/container.rs#L212-L227)). Um bit errado em qualquer ponto invalida o arquivo todo e **não localiza o dano**. A fidelidade final é o BLAKE3 de 256 bits do original, conferido no restauro ([container.rs:291-300](../src/container.rs#L291-L300)).
3. **Tudo em memória.** O corpo HTTP é `Bytes` (limite `SYNTRA_MAX_BODY_MB` = 4096) e a ingestão por diretório usa `std::fs::read` ([service.rs:428, 604, 621](../src/service.rs#L428)). `compress_blocking` recebe `Vec<u8>`, calcula o hash, classifica o arquivo inteiro, planeja, **descomprime tudo de novo para verificar** ([service.rs:472-489](../src/service.rs#L472-L489)) e só então grava ([service.rs:505-516](../src/service.rs#L505-L516)): na compressão convivem, no mínimo, o original, o payload e a cópia de verificação (≥3x o arquivo; inferência a partir do código, não medida).
4. **Decisão do plano por amostra única.** Acima de 48 MB a decisão usa uma janela contígua de 8 MiB do meio do arquivo (`probe_slice`, [planner.rs:470-476](../src/planner.rs#L470-L476)).
5. **Dedup antes de comprimir**, por BLAKE3 do arquivo inteiro ([service.rs:423-449](../src/service.rs#L423-L449)); o id do objeto é esse hash e faz parte do contrato da API.
6. **Risco latente a testar:** payloads entre 2 e 4 GiB num único campo protobuf. Não verifiquei que o `prost` os aceita; vira teste de aceite.

## 2. Evidência que motiva a mudança (medida)

- Bloco de **1 MiB** custa 0,3–1,3% de razão (zstd-1/3) contra 4,5–10,3% em 64 KiB (mix real); XML chega a 47–49% em 64 KiB com níveis altos.
- O caminho síncrono que cumpre 5 PB/mês é zstd-1/zstd-3 por bloco; incompressível detecta-se a ~5,5 GB/s por núcleo; o ótimo muda por classe (Seção 10.2 da pesquisa).
- A decisão por **bloco** (e não por arquivo) é pré-requisito do planejador por bloco e da recompressão assíncrona por M\*.

## 3. Decisão proposta

### 3.1 Formato v4 (quadro, não mensagem única)

```
cabeçalho   : magic "SYNT" | versão (u8 = 4) | flags (u8) | log2 do tamanho de bloco (u8)
blocos      : payload do bloco 0 | payload do bloco 1 | ...        (escritos em fluxo)
rodapé      : mensagem protobuf FooterV4 (ver 3.2)
final       : tamanho do rodapé (u32) | hash do rodapé (16 B) | magic "SYNT"
```

Envelopes v≤3 continuam sendo mensagens protobuf, **nunca começam com o magic "SYNT"**, e a leitura escolhe o decodificador pelo prefixo. Nenhum objeto antigo é reescrito; a migração acontece só por recompressão assíncrona quando valer (T3.2).

### 3.2 Rodapé

- `original_size`, `file_hash` (BLAKE3-256 do original inteiro, mantém o id do objeto), nome(s), MIME, `content_class`, tempos e métricas já existentes.
- **Tabela de planos** (poucos por objeto): cada plano = cadeia de `TransformStep`, `codec`, `codec_level`, e `DictionaryRef` (id + digest + tamanho), exatamente como hoje.
- **Tabela de blocos**: por bloco, `stored_len` (u32), `original_len` (u32, só difere do tamanho de bloco no último), `plan_idx` (u16) e `payload_hash` (BLAKE3 truncado a 128 bits **do payload comprimido**). Com 1 MiB e ~42 B por bloco, o rodapé de um arquivo de 5 GiB tem ~215 KB (0,004%).
- Opcional por objeto (flag `strict`): hash BLAKE3 do **bloco original**, para verificar fidelidade em leituras por faixa. **Decisão aberta O1** (ver 6).

### 3.3 Semântica

- **Cada bloco é decodificável sozinho** (contexto reiniciado). Erro de 1 bit contém-se em 1 bloco e é detectado pelo `payload_hash` **antes** de decodificar; o erro devolvido inclui o índice do bloco (`block_corrupted`).
- **Plano por bloco**: o planejador (T3.1) escolhe transform/codec/nível/dicionário por bloco; blocos incompressíveis ficam `stored` (T2.3). Efeito colateral útil: acaba o problema da decisão por amostra única (item 4 da Seção 1). **O planejador é simples, sem modelo aprendido** (resultado de T1.2: `zstd-3` fixo está a 1,1–1,6% do oráculo; o valor está em transform por bloco em dados numéricos, ganho de 9,4–11,6% em `científico`): *probe* + `zstd-3` + transform por amostra só em blocos numéricos.
- **Tamanho de bloco** por objeto (potência de 2): 1 MiB por padrão; objetos menores que um bloco são 1 bloco. A recompressão assíncrona pode **reagrupar em blocos maiores** (ex.: 16 MiB para xz-9e) quando o ganho compensar a perda de acesso aleatório.
- **Leitura por faixa** (`Range`) decodifica só os blocos necessários.

### 3.4 Fluxo (streaming)

- Compressão: lê blocos do corpo (`axum::body::Body`, não `Bytes`), mantém uma **janela limitada de blocos em voo** (ordem de `2 × threads`; backpressure), comprime em paralelo (Rayon), grava em arquivo temporário e atualiza o BLAKE3 do arquivo incrementalmente. Ao final grava o rodapé e faz `rename` atômico para o caminho derivado do `file_hash` (como `write_atomic` hoje).
- **Consequência de projeto:** o `file_hash` só é conhecido no fim do fluxo, então o dedup por arquivo inteiro deixa de poder evitar a compressão. Mitigação: cabeçalho opcional `x-content-hash` enviado pelo cliente; se o vault já tem esse hash, o motor **desvia** (não comprime), e **só confia depois de conferir o hash ao fim da leitura**.
- Verificação (*verify-on-write*): por bloco, em janela (decodifica o bloco recém-comprimido e compara com a entrada ainda em memória), e opcionalmente em outro nó (T4.3). Substitui a descompressão do arquivo inteiro.
- Memória de pico alvo: `janela × (bloco de entrada + bloco comprimido + contexto do codec)`. Referência medida (Python/libzstd, entrada de 32 MiB): zstd-3 44 MB, zstd-19 131 MB, xz-9e 363 MB por contexto; calibrar para o limite por nó (alvo inicial ≤512 MiB de RSS para um arquivo de 5 GiB).

## 4. Compatibilidade e migração

1. **Antes de tocar no código:** gerar um **corpus dourado** de envelopes v2 e v3 com o binário atual (arquivos pequenos de cada classe + hash do original esperado) e fixar em testes de restauro. Aceite: 100% restaurados após a mudança.
2. `parse` e `restore` passam a despachar por versão; v≤3 segue o caminho atual sem alteração de comportamento.
3. Rotas, `id` do objeto e campos de resposta **não mudam**; entram `x-block-size` (opcional), `Range` e o erro `block_corrupted`.
4. O ledger de uso não muda de formato; `cpu_ms` passa a ser a soma dos blocos.

## 5. Critérios de aceite

| # | Teste | Meta |
|---|---|---|
| A1 | Os 122 testes atuais | continuam verdes, **0 regressões** |
| A2 | Corpus dourado v2/v3 | 100% restaurado byte a byte |
| A3 | Round-trip v4 de todas as classes e esforços, incluindo vazio, 1 byte, tamanho não múltiplo do bloco e arquivo de exatamente N blocos | bit-exato; teste de propriedade (`proptest`) |
| A4 | Fuzz do parser v4 (cabeçalho, rodapé, tabelas) | sem pânico, sem alocação desproporcional |
| A5 | Corrupção injetada: 10⁴ bit flips aleatórios no payload, truncamentos do rodapé e de blocos | detecção 100%; dano contido a 1 bloco; erro indica o bloco |
| A6 | Arquivo de 5 GiB em fluxo | pico de RSS ≤ orçamento; vazão do zstd-3 por bloco ≥ 287 MB/s/núcleo (η = 0,42) ou ≥ 142 (η = 0,85) |
| A7 | Incompressível | ≥5 GB/s por núcleo no *probe*; razão exatamente 1,0 (+ rodapé) |
| A8 | Payload entre 2 e 4 GiB | funciona (ou falha com erro claro) no v≤3 e funciona no v4 |
| A9 | Determinismo: mesmo corpus em x86 Intel, x86 AMD e aarch64 | bytes comprimidos e reconstruídos idênticos (**V4**) |

## 6. Alternativas descartadas e questões abertas

- **Manter protobuf único com `repeated Block`:** continua carregando tudo na memória para (de)serializar e não permite escrita em fluxo; por isso o quadro com rodapé.
- **Arquivo de containers por tenant** (estilo `tar` de blobs): não resolve localização de dano nem streaming.
- **Blocos de 64 KiB por padrão:** custo de 4,5–10,3% (zstd-1/3) e até 49% em XML; só por classe/exigência de acesso aleatório fino.
- **O1 (aberta):** hash do **bloco original** por padrão ou só em `strict`? Custo: o restauro já faz BLAKE3 do arquivo inteiro; hash por bloco duplicaria esse custo. Depende de medir **BLAKE3 em Rust** (T1.5; o benchmark atual só mediu `blake2b`).
- **O2 (aberta):** redundância do rodapé (cópia no fim e no início por reescrita? ou delegar a erasure do vault, T4.2). Sem o rodapé o objeto é ilegível: precisa de proteção explícita.
- **O3 (aberta):** política de janela longa e `patch-from` (T3.3): entram como plano por bloco com referência a outro objeto? Depende de T1.6 (memória) e de definir o ciclo de vida do objeto referenciado (não pode ser apagado enquanto referenciado).
- **O4 (aberta):** tamanho padrão do bloco por classe, a decidir com T1.2 e com os dados do piloto.

## 7. Passos de implementação (cada um com testes e sem alterar comportamento existente)

1. Corpus dourado v2/v3 + testes de restauro (A2). Contêiner: comando da Seção 8 do plano.
2. Tipos e leitor/escritor de quadro v4 isolados (módulo novo), com A3 e A4, **sem ligar na API**.
3. Plano único por objeto, blocos fixos de 1 MiB, `zstd-3`/`stored` por bloco; ligar atrás de flag de configuração; A5, A7, A8.
4. Fluxo e janela limitada (A6), `Range` e `block_corrupted`.
5. Planejador por bloco (T3.1) e `verify` por bloco/nó (T2.5, T4.3).
6. Matriz de plataformas (A9).
