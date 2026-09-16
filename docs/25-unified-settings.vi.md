# Settings hợp nhất và plugin built-in cài sẵn

Phase 20 gom các cấu hình machine-local của Desktop vào một nơi duy nhất là **Settings**.

## Các phần trong Settings

### Workspace

Phần Workspace hiển thị Data Folder hiện tại, workspace identity, revision, trạng thái writer/read-only, thao tác device handoff và thao tác Change Data Folder đã có.

Dữ liệu project canonical vẫn nằm trong Data Root portable. Settings không tạo thêm database cấu hình song song.

### AI & Runtime

Phần này chứa cấu hình LLM Provider và OmniVoiceStudio runtime hiện có.

LLM Provider giữ nguyên cơ chế endpoint, model và tên environment variable chứa API key. OmniVoiceStudio giữ nguyên cơ chế chuẩn hóa runtime URL, kiểm tra health/capability và tên environment variable chứa bearer token.

Giá trị credential vẫn chỉ nằm trên máy và không đi vào Data Root/canonical project artifact. API key nhập trực tiếp ở card plugin được lưu trong credential file machine-local của Desktop để dùng lại sau khi restart; UI không đọc ngược secret ra input. Nhập key mới sẽ thay key đã lưu; Clear xóa key đã lưu và quay về environment variable nếu có.

Các action `Configure LLM` trong Review Center sẽ mở thẳng đúng phần Settings này.

### Plugins

Phần Plugins chứa Plugin Manager hiện tại với inventory, readiness, capability và lifecycle controls. Plugin cài thêm từ máy vẫn dùng luồng install/update/uninstall có guard và capability-impact preview như trước.

## Plugin built-in được cài sẵn

Desktop bundle đóng gói thư mục `plugins/` có sẵn trong repository vào application resource root dưới `plugins/`. Runtime hiện tại đã scan `resource_dir()/plugins`, vì vậy built-in plugin trong bản đóng gói vẫn đi qua đúng canonical `PluginRegistry`, không tạo registry thứ hai.

Built-in plugin mặc định được enable vì `PluginLifecycleStateV1` khởi tạo với danh sách disabled rỗng. Người dùng vẫn có thể disable built-in plugin trên từng máy. Built-in plugin tiếp tục là app-managed và không thể uninstall.

Các provider built-in hiện được đóng gói gồm:

- Generated Image API
- Generated Image Reference
- Pexels
- Pixabay
- Stick Figure Reference
- Storyblocks
- Unsplash

Việc cài sẵn plugin **không** đồng nghĩa nhúng credential của provider. Plugin cần API key hoặc tài khoản có license vẫn phải báo `SETUP REQUIRED` cho tới khi environment variable cần thiết có mặt trên máy.

## Ranh giới portability

Các dữ liệu sau vẫn là machine-local và không đi theo khi chuyển Data Root:

- endpoint/model/credential environment binding của LLM Provider
- runtime/tunnel URL và bearer-token environment binding của OmniVoiceStudio
- trạng thái enable/disable của plugin
- giá trị credential của provider
- binary/thư mục plugin do người dùng tự cài

Portable state của Project, WorkflowStep, Job, Attempt, ArtifactStore và Studio Pack không thay đổi.
