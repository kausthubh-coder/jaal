// Hooks in Chrome's own files. Each is defined in the Chrome file that reads
// it, and a null hook (or false) means stock Chrome. Chrome's file and our
// layer both include this header, so a changed signature fails to compile
// instead of crashing at the first call.
//
// Only forward declarations here: Chrome's files include this with
// `// nogncheck`, and it must not pull //jaal's dependencies into theirs.

#ifndef JAAL_HOOKS_H_
#define JAAL_HOOKS_H_

#include <memory>

class BrowserWindow;
class BrowserWindowInterface;
class ChromeBrowserMainParts;
struct BrowserWindowDeleter;

// chrome/browser/ui/webui_browser/webui_browser.cc: when true,
// IsWebUIBrowserEnabled() answers true in this process while the kWebium
// feature stays off. We get Chrome's guards for a browser without a
// BrowserView, without Webium's renderer add-on in every page.
extern bool g_jaal_webui_browser_enabled;

// chrome/browser/ui/views/frame/browser_window_factory.cc: asked first for
// every new browser window. Returning null falls back to Chrome's own window.
using JaalCreateBrowserWindowHook =
    std::unique_ptr<BrowserWindow, BrowserWindowDeleter> (*)(
        BrowserWindowInterface* browser,
        bool user_gesture,
        bool in_tab_dragging);
extern JaalCreateBrowserWindowHook g_jaal_create_browser_window;

// chrome/browser/chrome_browser_main.cc: lets jaal add its own startup parts
// to the browser process.
using JaalAddBrowserMainPartsHook = void (*)(ChromeBrowserMainParts* parts);
extern JaalAddBrowserMainPartsHook g_jaal_add_browser_main_parts;

#endif  // JAAL_HOOKS_H_
