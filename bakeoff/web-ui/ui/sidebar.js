// The sidebar. Rust owns the tabs and pushes them in with jaal.setTabs; we
// send user actions back as cefQuery messages (one line per field, see
// parse_command in src/lib.rs).

const ROW_H = 28;
// Rows drawn above and below the visible ones, so a fast scroll doesn't show
// blank space before the next scroll event.
const OVERSCAN = 8;

const urlField = document.getElementById("url");
const tabsEl = document.getElementById("tabs");
const rowsEl = document.getElementById("rows");
const spacer = document.getElementById("rows-spacer");
const rowWindow = document.getElementById("rows-window");
const rowCount = document.getElementById("row-count");

let rows = [];
let tabs = [];
let activeTab = null;
let firstDrawn = -1;
const rowEls = [];

function send(request) {
  window.cefQuery({ request, onSuccess() {}, onFailure() {} });
}

function epochMs(sinceOrigin) {
  return performance.timeOrigin + sinceOrigin;
}

// The one path a tab switch takes, from a click or from the bench.
function activateTab(id, startMs) {
  send(`activate\n${id}\n${startMs.toFixed(3)}`);
}

function drawRows() {
  const visible = Math.ceil(rowsEl.clientHeight / ROW_H) + OVERSCAN * 2;
  while (rowEls.length < visible) {
    const el = document.createElement("div");
    el.className = "row";
    el.append(document.createElement("span"), document.createElement("span"));
    el.firstChild.className = "t";
    el.lastChild.className = "u";
    rowWindow.append(el);
    rowEls.push(el);
  }
  const first = Math.max(0, Math.floor(rowsEl.scrollTop / ROW_H) - OVERSCAN);
  if (first === firstDrawn && rowEls.length === visible) return;
  firstDrawn = first;
  rowWindow.style.transform = `translateY(${first * ROW_H}px)`;
  for (let i = 0; i < rowEls.length; i++) {
    const row = rows[first + i];
    const el = rowEls[i];
    el.hidden = !row;
    if (row) {
      el.firstChild.textContent = row[0];
      el.lastChild.textContent = row[1];
    }
  }
}

function drawTabs() {
  tabsEl.replaceChildren(
    ...tabs.map((tab) => {
      const el = document.createElement("div");
      el.className = tab.id === activeTab ? "tab active" : "tab";
      el.textContent = tab.title || tab.url;
      el.title = tab.url;
      el.dataset.id = tab.id;
      return el;
    }),
  );
  const active = tabs.find((t) => t.id === activeTab);
  if (active && document.activeElement !== urlField) urlField.value = active.url;
}

// pointerdown, as Chrome's tab strip switches on press. The event's own
// timestamp is the click time tab_switch_start records.
tabsEl.addEventListener("pointerdown", (e) => {
  const el = e.target.closest(".tab");
  if (el) activateTab(Number(el.dataset.id), epochMs(e.timeStamp));
});

urlField.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    send(`navigate\n${urlField.value.replace(/\n/g, "")}`);
    urlField.blur();
  } else if (e.key === "Escape") {
    urlField.blur();
  }
});
urlField.addEventListener("focus", () => urlField.select());
urlField.addEventListener("blur", drawTabs);

rowsEl.addEventListener("scroll", drawRows, { passive: true });
window.addEventListener("resize", () => {
  firstDrawn = -1;
  drawRows();
});

window.jaal = {
  setRows(list) {
    rows = list;
    rowCount.textContent = rows.length.toLocaleString("en-US");
    spacer.style.height = `${rows.length * ROW_H}px`;
    firstDrawn = -1;
    drawRows();
  },

  setTabs(list, active) {
    tabs = list;
    activeTab = active;
    drawTabs();
  },

  scrollToRow(row) {
    rowsEl.scrollTop = row * ROW_H;
  },

  // Scrolls the rows a fixed step per frame for `ms`, then sends every
  // frame's time in one message so measuring adds no per-frame IPC.
  benchScroll(ms, pxPerFrame, collect) {
    const times = [];
    let start = null;
    const step = (t) => {
      if (start === null) start = t;
      if (collect) times.push(epochMs(t));
      rowsEl.scrollTop += pxPerFrame;
      if (t - start < ms) {
        requestAnimationFrame(step);
      } else {
        send(`frames\nscroll\n${times.map((x) => x.toFixed(3)).join(",")}`);
      }
    };
    requestAnimationFrame(step);
  },

  // Switches round-robin through `ids`, starting after the first (which is
  // active), `count` times, `gapMs` apart.
  benchSwitch(ids, count, gapMs) {
    let done = 0;
    const step = () => {
      activateTab(ids[(done + 1) % ids.length], epochMs(performance.now()));
      done++;
      if (done < count) setTimeout(step, gapMs);
    };
    step();
  },
};
