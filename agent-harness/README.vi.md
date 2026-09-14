# Gói tích hợp AI agent cho OmniCreator

Tài liệu chính và các lệnh cấu hình chi tiết nằm trong `agent-harness/README.md`. Bản tóm tắt này giúp kiểm tra nhanh cách dùng an toàn.

- Codex và Google Antigravity dùng skill tại `.agents/skills/omnicreator/SKILL.md`.
- Claude Code dùng skill tại `.claude/skills/omnicreator/SKILL.md`.
- Cả ba đều gọi cùng tiến trình local stdio: `omnicreator --data-root <path> mcp serve`.
- Nếu dùng LLM tự động, thêm `--llm-provider-config <path>` trước `mcp serve`; API key chỉ nằm trong biến môi trường của máy.
- Luôn inspect trạng thái trước khi mutate và inspect lại ngay sau mỗi mutation.
- Không sửa trực tiếp SQLite, ArtifactStore hoặc file nội bộ Data Root.
- Khi AUTO OFF hoặc provider/plugin/compute không sẵn sàng, dùng manual/external takeover tương ứng thay vì giả lập success.
- Chỉ báo hoàn tất khi OmniCreator xác nhận trạng thái canonical cuối cùng, ví dụ ProductionPack/export hợp lệ.

Các file mẫu cấu hình:

- Codex: `agent-harness/codex/config.toml.example`
- Claude Code: `agent-harness/claude/.mcp.json.example`
- Antigravity: `agent-harness/antigravity/mcp_config.json.example`
- OpenRouter: `agent-harness/providers/openrouter.json.example`
- LLMGateway: `agent-harness/providers/llmgateway.json.example`

Kịch bản recovery tham chiếu: `agent-harness/fixtures/blocked-workflow-recovery.json`.
