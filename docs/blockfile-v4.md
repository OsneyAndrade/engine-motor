# Formato v4 em blocos (`src/blockfile.rs`)

Estado em 2026-11-04: **implementado como módulo isolado (marco M1), não ligado à API, ao serviço, ao vault nem ao planejador** (isso é o M3). Especificação de origem: [ADR 001](adr-001-envelope-v4-blocos.md), Seções 3.1 a 3.3. O envelope v≤3 não muda. Testes e como rodá-los: [testes.md](testes.md).

## 1. Layout byte a byte

```
offset 0                      cabeçalho (7 bytes)
offset 7                      payload do bloco 0 | payload do bloco 1 | ... (stored_len[i] bytes cada)
offset 7 + Σ stored_len       rodapé: mensagem protobuf FooterV4 (footer_len bytes)
offset len - 24               final (24 bytes)
```

Tamanho do arquivo = `7 + Σ stored_len + footer_len + 24`. A igualdade é exigida na abertura.

### 1.1 Cabeçalho (7 bytes)

| Offset | Tam. | Campo | Valor |
|---|---|---|---|
| 0 | 4 | magic | `"SYNT"` (`53 59 4E 54`) |
| 4 | 1 | versão | `4`. Outro valor: `unsupported_version` |
| 5 | 1 | flags | `0`. Bit desconhecido ligado: `unsupported_flags`. Repetido no rodapé |
| 6 | 1 | log2 do tamanho de bloco | `12..=26` (4 KiB a 64 MiB); padrão do projeto: `20` (1 MiB). Repetido no rodapé |

O cabeçalho **não é coberto por nenhum hash**; por isso `flags` e `log2` são repetidos dentro do rodapé (que é), e a leitura recusa qualquer divergência. Efeito: alterar qualquer byte do cabeçalho vira erro tipado (comprovado pelo fuzz do teste `fuzz_do_parser_com_mais_de_vinte_mil_entradas`).

### 1.2 Payloads

Cada bloco é o resultado de `plano(bloco original)`, com contexto do codec reiniciado: **cada bloco decodifica sozinho**. Blocos sem ganho ficam `stored` (payload = original). O arquivo é escrito em fluxo: os payloads saem à medida que ficam prontos e o que sobra na memória é só a tabela de blocos (26 bytes por bloco) e o hash incremental.

### 1.3 Rodapé: `FooterV4` (`proto/engine.proto`, campos 1 a 19)

| Campo | Tipo | Conteúdo |
|---|---|---|
| 1 `header_flags` | uint32 | cópia das flags do cabeçalho |
| 2 `block_size_log2` | uint32 | cópia do log2 do cabeçalho |
| 3 `original_size` | uint64 | tamanho do original |
| 4 `file_hash` | bytes[32] | BLAKE3-256 do original inteiro (mesmo id de objeto do v≤3). Arquivo vazio: BLAKE3 da entrada vazia |
| 5 `plans` | repeated `BlockPlanV4` | tabela de planos (poucos por objeto), ver 1.4 |
| 6 `block_count` | uint32 | número de blocos = `ceil(original_size / tamanho_de_bloco)` |
| 7 `stored_lens` | bytes | `u32` LE por bloco: tamanho do payload |
| 8 `original_lens` | bytes | `u32` LE por bloco: tamanho do bloco original |
| 9 `plan_indices` | bytes | `u16` LE por bloco: índice na tabela de planos |
| 10 `payload_hashes` | bytes | 16 bytes por bloco: BLAKE3 do **payload comprimido** truncado a 128 bits |
| 11 a 19 | | metadados do `ProcessedFile`: `original_name`, `original_names`, `original_mime`, `content_class`, `plan_reason`, `verified`, `created_at`, `updated_at`, `file_metrics` |

As quatro tabelas de bloco são campos `bytes` de **largura fixa little-endian** (4 + 4 + 2 + 16 = 26 bytes por bloco). Para um arquivo de 5 GiB em blocos de 1 MiB: 5120 blocos, cerca de 133 KB de rodapé (0,0026%).

### 1.4 Plano de bloco: `BlockPlanV4`

`transforms` (cadeia de `TransformStep`), `codec`, `codec_level` e `dictionary` opcional (`DictionaryRef`: `id`, `digest` de 32 bytes, `size`). Em cada passo, `input_len` vale **sempre 0** (ver decisão D2).

### 1.5 Final (24 bytes)

| Offset (a partir do início do final) | Tam. | Campo |
|---|---|---|
| 0 | 4 | `footer_len` (u32 LE) |
| 4 | 16 | hash do rodapé: BLAKE3 dos `footer_len` bytes do rodapé, truncado a 128 bits |
| 20 | 4 | magic `"SYNT"` |

A leitura começa pelo final: lê 24 bytes, confere o magic, lê o rodapé, confere o hash, e só então interpreta o protobuf.

### 1.6 Detecção por prefixo

`is_v4(bytes)` testa os 4 primeiros bytes. Nenhum envelope v≤3 começa por `"SYNT"`: o primeiro byte `S` (0x53) seria a chave de campo 10 com tipo de fio 3 (início de grupo), que o `prost` nunca emite para `ProcessedFile` (sem campos de grupo). O teste `nenhum_processed_file_serializado_comeca_com_o_magic` cobre amostras e o teste `envelopes_v2_e_v3_do_corpus_dourado_nao_sao_v4` cobre os 61 envelopes do corpus. `container::parse` rejeita um v4 com `ContainerError::Decode` (`envelope_invalid`), sem pânico (teste `container_parse_recusa_v4_com_erro_claro_e_sem_panico`). O despacho `is_v4` → leitor correto é trabalho do M3.

## 2. Invariantes verificadas na abertura

Ordem das verificações (cada falha devolve um `BlockError` tipado antes de qualquer alocação proporcional a valores do arquivo):

1. Magic, versão, flags e log2 do cabeçalho (`bad_magic`, `unsupported_version`, `unsupported_flags`, `block_size_invalid`).
2. O arquivo comporta cabeçalho e final (`truncated`); o final termina em `"SYNT"` (`truncated`).
3. `footer_len <= 64 MiB` (`footer_too_large`) e `7 + footer_len + 24 <= tamanho do arquivo` (`footer_corrupted`).
4. Hash do rodapé confere (`footer_corrupted`).
5. Varredura de contagens no fio protobuf, **antes** do decode: no máximo 65 536 planos, 1024 nomes e 8 passos por plano (`footer_corrupted`). Sem isso um rodapé de 64 MiB de mensagens vazias viraria dezenas de milhões de structs.
6. Decode do `FooterV4`; `flags` e `log2` do rodapé iguais aos do cabeçalho; `file_hash` com 32 bytes.
7. Cada plano é válido (`plan_invalid`): codec conhecido e especificado, nível dentro da faixa do codec, no máximo 8 transforms conhecidos, largura em {1, 2, 4, 8} para `delta` e `byte_split`, `input_len == 0`, dicionário com id de 1 a 256 bytes e digest de 32 bytes, somente em codec que aceita dicionário.
8. `block_count <= MAX_BLOCKS` (2 540 780) e os quatro campos de tabela com exatamente `26 * block_count` bytes no total (`footer_corrupted`).
9. Geometria (`layout_invalid`): `ceil(original_size / bloco) == block_count`; todo bloco, exceto o último, tem `original_len == tamanho de bloco`; o último tem `1..=tamanho de bloco`; `plan_idx < nº de planos`; `stored_len <= original_len + original_len / 8 + 4096`; arquivo vazio tem 0 blocos e `file_hash` igual ao BLAKE3 do vazio.
10. `7 + Σ stored_len + footer_len + 24 == tamanho do arquivo` (`layout_invalid`).

A tabela de offsets (`u64` por bloco) é calculada aqui; `read_block(i)` não percorre nada.

## 3. Limites de segurança

| Limite | Valor | Onde vale |
|---|---|---|
| Tamanho do rodapé | 64 MiB (`MAX_FOOTER_LEN`) | leitura e escrita |
| Blocos por arquivo | 2 540 780 (`MAX_BLOCKS`, deixa 1 MiB do rodapé para planos e metadados) | leitura e escrita |
| log2 do bloco | 12 a 26 | leitura e escrita |
| Planos | 65 536 (cabe em `u16`) | leitura e escrita |
| Transforms por plano | 8 | leitura e escrita |
| Nomes originais | 1024 | leitura e escrita |
| `stored_len` | `original_len + original_len / 8 + 4096` | leitura e escrita |
| Payload lido por `read_block` | no máximo esse mesmo limite, independente do tamanho do arquivo | leitura |

Consequência: abrir um arquivo aloca no máximo O(tamanho do rodapé) (o rodapé inteiro cabe em 64 MiB e só é lido depois de caber dentro do arquivo); ler um bloco aloca no máximo `1,125 x bloco + 4 KiB` para o payload, mais a saída decodificada.

Proteções na decodificação de um bloco (antes de chamar `planner::restore`): o **hash do payload é conferido primeiro**; em `lz4`, o tamanho declarado no prefixo não pode passar de `original_len + 12,5% + 4 KiB`; em `zstd`, o frame precisa **declarar o tamanho do conteúdo** (o que `codec::encode` sempre faz) e esse valor respeita o mesmo limite. A intenção é que o buffer de decodificação do zstd fique limitado ao tamanho declarado (comportamento esperado do libzstd, **não medido** aqui).

## 4. API

| Item | Função |
|---|---|
| `is_v4(bytes)` | detecção por prefixo |
| `Writer::new(out, log2)` | escreve o cabeçalho na hora; `out: impl Write` |
| `Writer::add_plan(plano)` | valida, deduplica e devolve o índice (`u16`) |
| `Writer::push_block(original, plan_idx, payload)` | grava o payload, calcula o `payload_hash`, atualiza o BLAKE3 do arquivo; só o último bloco pode ser curto |
| `Writer::push_compressed(original, bloco)` | `add_plan` + `push_block` para a saída de `compress_block` |
| `Writer::finish(&Metadata)` | grava rodapé e final; devolve o `W` e um `Summary` (`file_hash`, `original_size`, `file_len`, `block_count`) |
| `Reader::from_bytes(&[u8])` / `Reader::from_reader(R: Read + Seek)` | abre lendo o final e o rodapé; o payload só é lido sob demanda |
| `block_count`, `block_size`, `original_size`, `file_hash`, `file_len`, `footer_len`, `plans`, `metadata`, `block_info(i)` | metadados e localização de cada bloco |
| `read_block(i, dicts)` | **verifica o `payload_hash` antes de decodificar**; o erro traz o índice |
| `restore_to(dicts, out)` / `restore_all(dicts)` | decodifica tudo, confere cada `original_len` e o `file_hash` |
| `verify_all()` | só hashes de payload, sem decodificar; para no primeiro bloco ruim |
| `corrupted_blocks()` | lista todos os blocos com hash divergente (base do *scrubbing*, T4.1) |
| `compress_block(original, &Plan, dict)` | usa `planner::execute`; **cai para `stored` quando o payload não é menor que o original**; devolve o plano efetivo e o payload |
| `decode_block(payload, &BlockPlan, dict, original_len)` | usa `planner::restore`; também serve para *verify-on-write* por bloco |

`restore_to` entrega os blocos ao `Write` à medida que decodifica; se o `file_hash` final divergir, o erro `fidelity_mismatch` chega depois de os bytes já terem saído. `restore_all` descarta tudo nesse caso. Quem usar `restore_to` num destino definitivo deve gravar em área temporária e promover só em caso de `Ok`.

A verificação do digest do dicionário é a de `container::restore`: o id vem do plano, o `digest` guardado precisa bater com o do dicionário resolvido (`dictionary_unavailable` / `dictionary_mismatch`, ambos com o índice do bloco).

### 4.1 Erros (`BlockError::code()`)

| Código | Quando |
|---|---|
| `bad_magic` | o início do arquivo não é `SYNT` |
| `unsupported_version` / `unsupported_flags` | versão diferente de 4 / flag desconhecida |
| `block_size_invalid` | log2 fora de 12..=26 |
| `truncated` | arquivo curto demais ou sem `SYNT` no final |
| `footer_too_large` | `footer_len` acima de 64 MiB |
| `footer_corrupted` | hash do rodapé, protobuf, contagens ou cópias do cabeçalho divergentes |
| `layout_invalid` | geometria ou soma de tamanhos incoerente |
| `plan_invalid` | plano da tabela fora das regras da Seção 2 |
| `block_out_of_range` | índice de bloco inexistente |
| `block_corrupted` | `payload_hash` do bloco não confere (traz o índice) |
| `dictionary_unavailable` / `dictionary_mismatch` | dicionário ausente / digest diferente (traz o índice) |
| `block_decode_failed` | codec ou transform falhou, ou o bloco saiu com tamanho diferente de `original_len` (traz o índice) |
| `fidelity_mismatch` | BLAKE3 do original reconstruído diverge do `file_hash` |
| `io_error` / `invalid_input` | falha do `Read`/`Write` / uso inválido do escritor |

`BlockError::block_index()` devolve o índice nos erros por bloco; a mensagem contém `bloco N`.

## 5. Decisões e desvios do ADR 001

| # | Decisão | Motivo |
|---|---|---|
| D1 | Tabela de blocos em campos `bytes` de largura fixa, não em varints empacotados | 26 bytes por bloco (o ADR estimava ~42), acesso O(1) sem decodificar e nenhum custo de alocação por elemento no `prost` |
| D2 | `TransformStep.input_len` é 0 nos planos do v4 | o plano é compartilhado por vários blocos e `input_len` é por bloco (muda no último e em `rle`/`csv_columnar`). Com 0, `transform::invert` pula a checagem interna e a verificação passa a ser o `original_len` do bloco, checado depois da inversão. Também elimina uma dica de capacidade controlada pelo atacante |
| D3 | `flags` e `log2` repetidos no rodapé | o cabeçalho não tem hash; assim toda alteração dele é detectada |
| D4 | Geometria estrita: todo bloco, menos o último, tem exatamente o tamanho de bloco | é o que o ADR diz ("só difere no último"); dá deslocamento original = `i * bloco` para `Range`, impede rodapés com milhares de blocos minúsculos e endurece o parser. **Custo:** `csv_columnar` só se aplica a um bloco cujos limites caem em fim de linha (ver Seção 6) |
| D5 | `stored_len` limitado a `original_len + 12,5% + 4 KiB` | torna a alocação por bloco independente do tamanho do arquivo. `compress_block` nunca passa de `original_len` |
| D6 | Zstd em blocos precisa declarar o tamanho do conteúdo | permite recusar o bloco antes de decodificar e, pelo comportamento esperado do libzstd (não medido), limita o buffer do decodificador; já é o comportamento de `codec::encode` |
| D7 | Metadados: entram nome(s), MIME, classe, motivo do plano, `verified`, tempos e `file_metrics`; **não entram** `ref_count`, `pointer_to`, `envelope_checksum` e `processed_size` | `ref_count` e `pointer_to` são estado do vault, não do conteúdo; `processed_size` é derivável (`file_len`); o `envelope_checksum` de 64 bits é substituído pelos hashes por bloco e do rodapé |
| D8 | `is_v4` testa só o magic | um arquivo com magic e versão futura chega ao leitor v4, que responde `unsupported_version` em vez de cair no parser v3 |
| D9 | Reaproveita `planner::execute`, `planner::restore` e `DictionaryResolver` sem alterá-los | exigência do marco. Nenhum arquivo existente do núcleo foi tocado |

Questões abertas do ADR que **continuam abertas**: **O1** (hash do bloco original; só existe o `file_hash`, que o restauro confere de qualquer forma), **O2** (o rodapé é único, sem cópia; sem ele o objeto é ilegível), **O3** (referência a outro objeto) e **O4** (tamanho de bloco por classe). Não há flag `strict`.

## 6. Riscos conhecidos e pendências (para o M3)

1. **Bomba de descompressão em `brotli`, `xz`, `bzip2` e `deflate`: resolvida no M2b** (a decisão D9 acima vale para o marco M1; o M2b alterou `codec.rs`, `planner.rs`, `transform.rs` e `container.rs`, não este módulo). `decode_block` chama `planner::restore` com `Some(original_len)`, que agora usa o teto `min(original_len, 64 GiB)` em todos os codecs (`codec::decode_limited`). O teste `bloco_v4_usa_o_original_len_como_teto_em_todos_os_codecs_sem_tamanho_declarado` usa bombas de 64 MiB e `bloco_v4_recusa_bomba_real_de_1_gib_em_todos_os_codecs` usa as de 1 GiB: `zstd` é recusado pelo tamanho declarado, `xz` e `bzip2` são cortados em 1 MiB (pico de alocação do Rust de cerca de 1,2 MB, 2 a 5 ms) e `lz4`, `brotli` e `deflate` nem entram no arquivo, porque o payload de 1 GiB de zeros passa de `stored_len` máximo do bloco e o escritor o recusa (o leitor também recusaria). O erro no v4 é `block_decode_failed`.
2. **Defeito de `transform::csv_invert`: resolvido no M2b.** A soma `p + len` é verificada (`checked_add`), as dimensões colunares são limitadas e a inversão tem teto de saída. Os testes de robustez cobrem 25.000 casos por transform e por mutação do corpus dourado sem pânico; o mesmo vale para o v≤3. Os fuzzers do v4 continuam sem gerar planos `csv_columnar`; a cobertura de `csv_columnar` hostil está em `robustez_tests`.
3. **`csv_columnar` com geometria fixa.** Só funciona em blocos com linhas inteiras e colunas uniformes; um corte no meio de uma linha faz `csv_apply` falhar e `compress_block` devolve o erro (o chamador decide). Para CSV real o M4 precisará de blocos alinhados a registro (flag nova de formato) ou de aplicar o transform ao arquivo antes de blocar.
4. **Rodapé sem redundância (O2).** Corrupção do rodapé é detectada, mas não reparada.
5. **O *probe* trata o bloco isolado.** Blocos numéricos suaves passam (teste `numerico_suave_nao_e_descartado`), mas dados cuja economia só aparece depois de um transform e fica abaixo de 2% sem ele seriam marcados como incompressíveis; o M4 deve decidir a ordem *probe* e transform.

## 7. Como evoluir

- **Nova versão do formato:** troque o byte de versão (5). Leitores v4 devolvem `unsupported_version`; `is_v4` continua roteando pelo magic.
- **Recurso opcional:** use um bit de `flags` (hoje nenhum está definido). O bit precisa estar também em `header_flags`; leitores antigos o recusam com `unsupported_flags`. Candidatos: blocos de tamanho variável (CSV por registro), hash do bloco original (O1), cópia do rodapé (O2).
- **Campos novos no rodapé:** acrescente números novos em `FooterV4`/`BlockPlanV4` (nunca renumere). O protobuf ignora campos desconhecidos, mas eles entram no hash do rodapé; o que mudar a semântica deve vir com bit de flag.
- **Novos codecs e transforms:** entram pelos enums `Codec` e `Transform` existentes; ajuste `BlockPlan::validate` se o novo item tiver regras de nível ou largura.
- **Ligar à API (M3):** despachar por `is_v4`; `Writer` sobre arquivo temporário e `rename` atômico; janela de blocos em voo com Rayon chamando `compress_block` e entregando em ordem a `push_compressed`; `Range` a partir de `block_info` (`offset original = i * block_size()`); mapear `BlockError::code()` para o corpo de erro.
