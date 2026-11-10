#ifndef JAAL_MAIN_DELEGATE_H_
#define JAAL_MAIN_DELEGATE_H_

#include <optional>

// nogncheck: chrome_dll compiles ChromeMainDelegate itself, so there's no
// target to depend on.
#include "chrome/app/chrome_main_delegate.h"  // nogncheck

namespace jaal {

// Exit codes past Chrome's own (chrome_result_codes.h ends at 43).
inline constexpr int kExitUiLibraryMissing = 90;
inline constexpr int kExitUiLibraryAbiMismatch = 91;

// ChromeMain creates this instead of ChromeMainDelegate. In the browser
// process it installs jaal's hooks, makes Chrome start with no window and no
// first-run UI, and loads the app's UI library before any profile opens.
// Helper processes run Chrome's delegate unchanged.
class JaalMainDelegate : public ChromeMainDelegate {
 public:
  explicit JaalMainDelegate(const StartupTimestamps& timestamps);
  JaalMainDelegate(const JaalMainDelegate&) = delete;
  JaalMainDelegate& operator=(const JaalMainDelegate&) = delete;
  ~JaalMainDelegate() override;

  // content::ContentMainDelegate:
  std::optional<int> BasicStartupComplete() override;
  std::optional<int> PreBrowserMain() override;
};

}  // namespace jaal

#endif  // JAAL_MAIN_DELEGATE_H_
