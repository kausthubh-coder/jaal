#include "jaal/main_delegate.h"

#include <memory>
#include <string>

#include "base/base_paths.h"
#include "base/command_line.h"
#include "base/files/file_path.h"
#include "base/functional/bind.h"
#include "base/logging.h"
#include "base/native_library.h"
#include "base/path_service.h"
#include "base/task/single_thread_task_runner.h"
#include "build/build_config.h"
#include "chrome/browser/chrome_browser_main.h"
#include "chrome/browser/chrome_browser_main_extra_parts.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface.h"
#include "chrome/browser/ui/browser_window_deleter.h"
#include "chrome/common/chrome_switches.h"
#include "content/public/common/content_switches.h"
#include "jaal/browser_window.h"
#include "jaal/engine.h"
#include "jaal/hooks.h"
#include "jaal/public/jaal_engine.h"

namespace jaal {

namespace {

// The UI library sits next to the engine library (chrome.dll on Windows). The
// Mac path inside the framework comes with the Mac build.
#if BUILDFLAG(IS_WIN)
constexpr base::FilePath::CharType kUiLibraryName[] =
    FILE_PATH_LITERAL("jaal_ui.dll");
#elif BUILDFLAG(IS_MAC)
constexpr base::FilePath::CharType kUiLibraryName[] =
    FILE_PATH_LITERAL("libjaal_ui.dylib");
#endif

// Loaded once in the browser process and never unloaded: GPUI keeps code and
// statics in it for the life of the process.
base::NativeLibrary g_ui_library = nullptr;
jaal_ui_main_fn g_ui_main = nullptr;

std::unique_ptr<BrowserWindow, BrowserWindowDeleter> CreateJaalBrowserWindow(
    BrowserWindowInterface* browser,
    bool user_gesture,
    bool in_tab_dragging) {
  // Normal windows are the app's. Popups, app windows, DevTools and PiP keep
  // Chrome's own window until M4 decides each of them.
  if (browser->GetType() != BrowserWindowInterface::Type::TYPE_NORMAL) {
    return nullptr;
  }
  return std::unique_ptr<BrowserWindow, BrowserWindowDeleter>(
      new JaalBrowserWindow(browser));
}

// Runs the app's UI once the browser has started.
class JaalBrowserMainParts : public ChromeBrowserMainExtraParts {
 public:
  JaalBrowserMainParts() = default;
  JaalBrowserMainParts(const JaalBrowserMainParts&) = delete;
  JaalBrowserMainParts& operator=(const JaalBrowserMainParts&) = delete;
  ~JaalBrowserMainParts() override = default;

  // ChromeBrowserMainExtraParts:
  void PreBrowserStart() override { Engine::Get().OnBrowserStarting(); }

  void PostBrowserStart() override {
    // Posted, so the UI starts from the top of the loop like everything else
    // the engine sends it.
    base::SingleThreadTaskRunner::GetCurrentDefault()->PostTask(
        FROM_HERE, base::BindOnce([] { Engine::Get().StartUi(g_ui_main); }));
  }

  void PostMainMessageLoopRun() override { Engine::Get().OnMainLoopEnded(); }
};

void AddBrowserMainParts(ChromeBrowserMainParts* parts) {
  parts->AddParts(std::make_unique<JaalBrowserMainParts>());
}

}  // namespace

JaalMainDelegate::JaalMainDelegate(const StartupTimestamps& timestamps)
    : ChromeMainDelegate(timestamps) {}

JaalMainDelegate::~JaalMainDelegate() = default;

std::optional<int> JaalMainDelegate::BasicStartupComplete() {
  base::CommandLine* command_line = base::CommandLine::ForCurrentProcess();
  const bool is_browser =
      command_line->GetSwitchValueASCII(switches::kProcessType).empty();
  if (is_browser) {
    // The app opens windows itself, and jaal shows none of Chrome's first-run
    // or default-browser UI.
    command_line->AppendSwitch(switches::kNoFirstRun);
    command_line->AppendSwitch(switches::kNoDefaultBrowserCheck);
    command_line->AppendSwitch(switches::kNoStartupWindow);

    g_jaal_webui_browser_enabled = true;
    g_jaal_create_browser_window = &CreateJaalBrowserWindow;
    g_jaal_add_browser_main_parts = &AddBrowserMainParts;
  }
  return ChromeMainDelegate::BasicStartupComplete();
}

std::optional<int> JaalMainDelegate::PreBrowserMain() {
  std::optional<int> exit_code = ChromeMainDelegate::PreBrowserMain();
  if (exit_code.has_value()) {
    return exit_code;
  }

  // Before any profile opens, so a wrong UI library never touches user data.
  base::FilePath dir;
  CHECK(base::PathService::Get(base::DIR_MODULE, &dir));
  const base::FilePath path = dir.Append(kUiLibraryName);
  base::NativeLibraryLoadError error;
  g_ui_library = base::LoadNativeLibrary(path, &error);
  if (!g_ui_library) {
    LOG(ERROR) << "jaal: can't load the UI library " << path << ": "
               << error.ToString();
    return kExitUiLibraryMissing;
  }
  auto abi_major = reinterpret_cast<jaal_ui_abi_major_fn>(
      base::GetFunctionPointerFromNativeLibrary(g_ui_library,
                                                "jaal_ui_abi_major"));
  g_ui_main = reinterpret_cast<jaal_ui_main_fn>(
      base::GetFunctionPointerFromNativeLibrary(g_ui_library, "jaal_ui_main"));
  if (!abi_major || !g_ui_main) {
    LOG(ERROR) << "jaal: " << path
               << " doesn't export jaal_ui_abi_major and jaal_ui_main";
    return kExitUiLibraryAbiMismatch;
  }
  if (abi_major() != JAAL_ENGINE_ABI_MAJOR) {
    LOG(ERROR) << "jaal: " << path << " is built for engine ABI "
               << abi_major() << "; this engine is " << JAAL_ENGINE_ABI_MAJOR;
    return kExitUiLibraryAbiMismatch;
  }
  return std::nullopt;
}

}  // namespace jaal
