#ifndef JAAL_ENGINE_H_INTERNAL_
#define JAAL_ENGINE_H_INTERNAL_

#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "base/callback_list.h"
#include "base/memory/raw_ptr.h"
#include "base/memory/weak_ptr.h"
#include "base/scoped_observation.h"
#include "chrome/browser/ui/browser_window/public/browser_collection.h"
#include "chrome/browser/ui/browser_window/public/browser_collection_observer.h"
#include "chrome/browser/ui/tabs/tab_strip_model_observer.h"
#include "jaal/public/jaal_engine.h"

class ScopedKeepAlive;

namespace tabs {
class TabInterface;
}

namespace jaal {

class JaalBrowserWindow;

// Maps objects to jaal_handle values: a slot plus a generation, so a handle
// used after its object is gone answers JAAL_ERR_GONE instead of reaching
// whatever reused the slot.
template <typename T>
class HandleRegistry {
 public:
  jaal_handle Add(T* object) {
    auto it = slot_of_.find(object);
    if (it != slot_of_.end()) {
      return {it->second, entries_[it->second].generation};
    }
    uint32_t slot;
    if (!free_slots_.empty()) {
      slot = free_slots_.back();
      free_slots_.pop_back();
    } else {
      // Slot 0 stays unused so the zero handle names nothing.
      if (entries_.empty()) {
        entries_.emplace_back();
      }
      slot = static_cast<uint32_t>(entries_.size());
      entries_.emplace_back();
    }
    entries_[slot].object = object;
    entries_[slot].generation += 1;
    slot_of_[object] = slot;
    return {slot, entries_[slot].generation};
  }

  // Returns the handle the object had, or the zero handle.
  jaal_handle Remove(T* object) {
    auto it = slot_of_.find(object);
    if (it == slot_of_.end()) {
      return {0, 0};
    }
    const uint32_t slot = it->second;
    const jaal_handle handle = {slot, entries_[slot].generation};
    entries_[slot].object = nullptr;
    free_slots_.push_back(slot);
    slot_of_.erase(it);
    return handle;
  }

  T* Get(jaal_handle handle) const {
    if (handle.slot == 0 || handle.slot >= entries_.size()) {
      return nullptr;
    }
    const Entry& entry = entries_[handle.slot];
    return entry.generation == handle.generation ? entry.object.get() : nullptr;
  }

  std::optional<jaal_handle> Find(T* object) const {
    auto it = slot_of_.find(object);
    if (it == slot_of_.end()) {
      return std::nullopt;
    }
    return jaal_handle{it->second, entries_[it->second].generation};
  }

  size_t size() const { return slot_of_.size(); }

 private:
  struct Entry {
    raw_ptr<T> object = nullptr;
    uint32_t generation = 0;
  };
  std::vector<Entry> entries_;
  std::vector<uint32_t> free_slots_;
  std::map<T*, uint32_t> slot_of_;
};

// The engine side of jaal_engine.h: owns the handles, the event sink, the
// keep-alive and the quit state. Lives on the UI thread for the whole browser
// process.
class Engine : public TabStripModelObserver, public BrowserCollectionObserver {
 public:
  static Engine& Get();

  Engine(const Engine&) = delete;
  Engine& operator=(const Engine&) = delete;

  // Called from the browser process's startup parts.
  void OnBrowserStarting();
  void StartUi(jaal_ui_main_fn ui_main);
  void OnMainLoopEnded();

  // Called by JaalBrowserWindow.
  void OnWindowCreated(JaalBrowserWindow* window);
  void OnWindowDestroyed(JaalBrowserWindow* window);

  // The C API, one method per call.
  jaal_status SetEventSink(jaal_event_sink sink, void* user_data);
  jaal_status Quit();
  jaal_status WindowCreate(jaal_handle* out_window);
  jaal_status WindowClose(jaal_handle window);
  jaal_status WindowNativeHandle(jaal_handle window, void** out_handle);
  jaal_status WindowSetPageRect(jaal_handle window, jaal_rect rect);
  jaal_status TabOpen(jaal_handle window,
                      const char* url,
                      uint8_t foreground,
                      jaal_handle* out_tab);
  jaal_status TabClose(jaal_handle tab);
  jaal_status TabActivate(jaal_handle tab);
  jaal_status TabLoad(jaal_handle tab, const char* url);

  static const jaal_engine_api* GetApi(uint32_t abi_major,
                                       uint32_t struct_size);

 private:
  friend class base::NoDestructor<Engine>;

  // What an event carries until it is delivered. Strings are owned here and
  // lent to the app only for the callback.
  struct PendingEvent {
    jaal_event_kind kind;
    jaal_handle window = {0, 0};
    jaal_handle tab = {0, 0};
    std::string url;
    std::string title;
    bool loading = false;
    bool active = false;
  };

  Engine();
  ~Engine() override;

  // TabStripModelObserver:
  void OnTabStripModelChanged(TabStripModel* tab_strip_model,
                              const TabStripModelChange& change,
                              const TabStripSelectionChange& selection) override;
  void OnTabChangedAt(tabs::TabInterface* tab,
                      TabChangeType change_type) override;

  // BrowserCollectionObserver:
  void OnBrowserClosed(BrowserWindowInterface* browser) override;

  // Never delivers synchronously: the app must not see a callback while it is
  // inside a call.
  void PostEvent(PendingEvent event);
  void DeliverEvent(PendingEvent event);
  PendingEvent TabStateEvent(tabs::TabInterface* tab, jaal_handle window);
  std::optional<jaal_handle> WindowHandleForTab(tabs::TabInterface* tab);

  void OnClosingAllBrowsers(bool closing);
  void MaybeFinishQuit();

  jaal_event_sink sink_ = nullptr;
  raw_ptr<void> sink_user_data_ = nullptr;

  HandleRegistry<JaalBrowserWindow> windows_;
  HandleRegistry<tabs::TabInterface> tabs_;

  // Holds the browser process alive with no windows, the job Chrome's
  // AppController does on the Mac. Dropped only once a quit is committed.
  std::unique_ptr<ScopedKeepAlive> keep_alive_;
  bool quitting_ = false;
  bool will_quit_sent_ = false;
  base::CallbackListSubscription closing_all_browsers_subscription_;
  base::ScopedObservation<BrowserCollection, BrowserCollectionObserver>
      browser_collection_observation_{this};

  base::WeakPtrFactory<Engine> weak_factory_{this};
};

}  // namespace jaal

#endif  // JAAL_ENGINE_H_INTERNAL_
