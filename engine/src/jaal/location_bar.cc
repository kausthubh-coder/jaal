#include "jaal/location_bar.h"

#include "chrome/browser/ui/browser_command_controller.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface.h"
#include "chrome/browser/ui/tabs/tab_strip_model.h"

namespace jaal {

JaalLocationBar::JaalLocationBar(BrowserWindowInterface* browser)
    : LocationBar(chrome::BrowserCommandController::From(browser)),
      browser_(browser) {}

JaalLocationBar::~JaalLocationBar() = default;

void JaalLocationBar::FocusLocation(bool is_user_initiated,
                                    bool clear_focus_if_failed) {}
void JaalLocationBar::FocusSearch() {}
void JaalLocationBar::UpdateFocusBehavior(bool toolbar_visible) {}
void JaalLocationBar::UpdateContentSettingsIcons() {}
void JaalLocationBar::SaveStateToContents(content::WebContents* contents) {}
void JaalLocationBar::Revert() {}

OmniboxView* JaalLocationBar::GetOmniboxView() {
  return nullptr;
}

OmniboxPopupView* JaalLocationBar::GetOmniboxPopupView() {
  return nullptr;
}

OmniboxController* JaalLocationBar::GetOmniboxController() {
  return nullptr;
}

bool JaalLocationBar::ShouldCloseOmniboxPopup(ui::MouseEvent* event) {
  return false;
}

ChipController* JaalLocationBar::GetChipController() {
  return nullptr;
}

content::WebContents* JaalLocationBar::GetWebContents() {
  return browser_->GetTabStripModel()->GetActiveWebContents();
}

LocationBarModel* JaalLocationBar::GetLocationBarModel() {
  return nullptr;
}

std::optional<bubble_anchor_util::AnchorConfiguration>
JaalLocationBar::GetChipAnchor() {
  return std::nullopt;
}

ui::TrackedElement* JaalLocationBar::GetAnchorOrNull() {
  return nullptr;
}

BrowserWindowInterface* JaalLocationBar::GetBrowser() {
  return browser_;
}

Profile* JaalLocationBar::GetProfile() {
  return browser_->GetProfile();
}

void JaalLocationBar::OnChanged() {}
void JaalLocationBar::UpdateWithoutTabRestore() {}

bool JaalLocationBar::IsInitialized() const {
  return true;
}

// Not visible and not drawn, so Chrome anchors its bubbles elsewhere.
bool JaalLocationBar::IsVisible() const {
  return false;
}

bool JaalLocationBar::IsDrawn() const {
  return false;
}

bool JaalLocationBar::IsFullscreen() const {
  return false;
}

bool JaalLocationBar::IsEditingOrEmpty() const {
  return false;
}

bool JaalLocationBar::IsMouseHovered() const {
  return false;
}

bool JaalLocationBar::IsFocusWithin() const {
  return false;
}

void JaalLocationBar::InvalidateLayout() {}

gfx::Rect JaalLocationBar::Bounds() const {
  return gfx::Rect();
}

gfx::Rect JaalLocationBar::BoundsInScreen() const {
  return gfx::Rect();
}

gfx::Size JaalLocationBar::MinimumSize() const {
  return gfx::Size();
}

gfx::Size JaalLocationBar::PreferredSize() const {
  return gfx::Size();
}

void JaalLocationBar::Update(content::WebContents* contents) {}
void JaalLocationBar::ResetTabState(content::WebContents* contents) {}

bool JaalLocationBar::HasSecurityStateChanged() {
  return false;
}

LocationBarTesting* JaalLocationBar::GetLocationBarForTesting() {
  return nullptr;
}

void JaalLocationBar::AnnounceAlert(const std::u16string& announcement) {}

}  // namespace jaal
