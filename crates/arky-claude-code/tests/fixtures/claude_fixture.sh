#!/bin/sh
set -eu

if [ "${1-}" = "--version" ]; then
  printf '%s\n' "${CLAUDE_FIXTURE_VERSION:-claude-fixture 1.0.0}"
  exit 0
fi

SESSION_ID="${CLAUDE_FIXTURE_SESSION_ID:-fixture-session}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --session-id)
      SESSION_ID="$2"
      shift 2
      ;;
    --output-format|--model)
      shift 2
      ;;
    --print|--verbose)
      shift 1
      ;;
    *)
      shift 1
      ;;
  esac
done

if [ -n "${CLAUDE_FIXTURE_STDERR:-}" ]; then
  printf '%s\n' "$CLAUDE_FIXTURE_STDERR" >&2
fi

MODE="${CLAUDE_FIXTURE_MODE:-contract_basic}"
case "$MODE" in
  contract_basic)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":11,\"output_tokens\":5,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    ;;
  tool_cycle)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"stream_event\",\"session_id\":\"$SESSION_ID\",\"event\":{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tool-1\",\"name\":\"search\",\"input\":{}}}}"
    printf '%s\n' "{\"type\":\"stream_event\",\"session_id\":\"$SESSION_ID\",\"event\":{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\\\"docs\\\"}\"}}}"
    printf '%s\n' "{\"type\":\"stream_event\",\"session_id\":\"$SESSION_ID\",\"event\":{\"type\":\"content_block_stop\",\"index\":0}}"
    printf '%s\n' "{\"type\":\"user\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"name\":\"search\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}],\"is_error\":false}]}}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"after tool\"}]}}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":12,\"output_tokens\":8,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    ;;
  wait_for_stdin_close)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"stdin closed\"}]}}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":2,\"output_tokens\":2,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    cat >/dev/null
    ;;
  rate_limit_event)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"rate limit metadata should not abort the turn\"}]}}"
    printf '%s\n' "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"allowed\",\"resetsAt\":1773680400,\"rateLimitType\":\"five_hour\",\"overageStatus\":\"rejected\",\"overageDisabledReason\":\"out_of_credits\",\"isUsingOverage\":false},\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"is_error\":false,\"usage\":{\"input_tokens\":2,\"output_tokens\":8,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    ;;
  nested_preview)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"parent-1\",\"name\":\"Task\",\"input\":{}}]}}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":\"parent-1\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"child-1\",\"name\":\"Read\",\"input\":{\"path\":\"README.md\"},\"parent_tool_use_id\":\"parent-1\"}]}}"
    printf '%s\n' "{\"type\":\"user\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":\"parent-1\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"child-1\",\"name\":\"Read\",\"content\":[{\"type\":\"text\",\"text\":\"child-result\"}],\"is_error\":false}]}}"
    printf '%s\n' "{\"type\":\"user\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"parent-1\",\"name\":\"Task\",\"content\":[{\"type\":\"text\",\"text\":\"parent-result\"}],\"is_error\":false}]}}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":5,\"output_tokens\":5,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    ;;
  auth_failed)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Failed to authenticate. API Error: 401 token expired\"}]},\"error\":\"authentication_failed\"}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"stop_sequence\",\"is_error\":true,\"result\":\"Failed to authenticate. API Error: 401 token expired\",\"usage\":{\"input_tokens\":0,\"output_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    exit 1
    ;;
  truncated_stream)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"$(printf 'x%.0s' $(seq 1 600))\"}]}}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"unterminated}"
    ;;
  malformed)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{not json"
    ;;
  crash_after_first_event)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' 'fixture crashed' >&2
    exit 7
    ;;
  *)
    printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$SESSION_ID\"}"
    printf '%s\n' "{\"type\":\"assistant\",\"session_id\":\"$SESSION_ID\",\"parent_tool_use_id\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"$MODE\"}]}}"
    printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0},\"session_id\":\"$SESSION_ID\"}"
    ;;
esac
