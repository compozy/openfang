# Task 4R: Bridge Arky -> LlmDriver em `openfang-provider-binding`

## Summary

- Implementar o Task 4R com escopo estrito ao bridge, factory e conversões no crate `openfang-provider-binding`.
- Não alterar `openfang-runtime::drivers::create_driver()`, `agent_loop.rs` nem adicionar wiring no kernel nesta entrega.
- Seguir o contrato real do repo: os providers Arky atuais expõem thinking e tool lifecycle principalmente via `AgentEvent`, então `complete()` e `stream()` do bridge devem compartilhar um coletor baseado em `Provider::stream()`.

## Key Changes

- Adicionar os módulos `bridge`, `convert` e `instantiate` ao crate `openfang-provider-binding` e reexportar os símbolos públicos pelo `lib.rs`.
- Expor `ArkyDriverBridge`, `binding_to_driver(binding, install) -> Result<Arc<dyn LlmDriver>, BridgeError>` e `instantiate_provider(config) -> Result<Arc<dyn Provider>, InstantiateError>`.
- Fazer `complete()` e `stream()` chamarem o mesmo fluxo interno de coleta baseado em `Provider::stream()`, não `Provider::generate()`.
- Criar um estado interno de coleta para acumular mensagem terminal, reasoning, tool calls, input fragments, tool results, `usage` e `finish_reason`.
- Implementar `completion_request_to_provider` com prepend de `system`, conversão de mensagens e tools, mapeamento de `max_tokens`, `temperature` e `thinking.budget_tokens`.
- Converter imagens OpenFang <-> Arky com base64, adicionando `base64` ao crate.
- Implementar `instantiate_provider` com os construtores reais do repo:
  - `CodexProvider::with_config`
  - `ClaudeCodeProvider::with_config`
  - wrappers concretos (`BedrockProvider`, `ZaiProvider`, `OpenRouterProvider`, `VercelProvider`, `MoonshotProvider`, `MinimaxProvider`, `VertexProvider`, `OllamaProvider`)
- Fazer `binding_to_driver` compor `build_provider_config -> instantiate_provider -> ArkyDriverBridge::new`.
- Atualizar `scripts/check-deps.sh` para incluir `openfang-provider-binding` e `openfang-agent-definition`.

## Test Plan

- Cobrir `convert.rs` com mensagens textuais e estruturadas, imagens, flatten de `ToolResult.content`, prepend de system, reasoning, finish reasons e provider errors.
- Cobrir `bridge.rs` com fake providers implementando `Provider` para validar `complete()`, `stream()`, síntese de resposta final e erro por ausência de mensagem terminal.
- Cobrir `instantiate.rs` com construção real dos providers suportados, sem rede nem binário.
- Cobrir `binding_to_driver()` com criação para Codex, Claude Code e pelo menos um wrapper compatível, além de erros tipados.
- Rodar `cargo test -p openfang-provider-binding`, `./scripts/check-deps.sh`, depois `make fmt`, `make lint` e `make test`.

## Assumptions And Defaults

- O escopo aprovado é somente Task 4R; o primeiro caller real no kernel/control-plane fica para tasks posteriores.
- Não adicionar dependência inútil de `openfang-kernel -> openfang-provider-binding` nesta task.
- O bridge seguirá o contrato real dos events Arky do repo quando o markdown do task divergir do código atual.
- `Provider::generate()` não será a fonte principal de verdade no bridge porque hoje ele não preserva thinking o suficiente para o contrato do OpenFang.
