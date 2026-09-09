// 画中画窗口内的倒计时引擎。
//
// 由门户页面在 Document Picture-in-Picture 窗口创建后注入：
// 门户先把卡片 DOM、样式表和 __hyzPip* 全局参数放进画中画窗口，
// 再以 <script src="/pip-countdown.js"> 加载本文件。脚本在画中画
// 窗口自己的上下文里每秒重算并更新 DOM，因此主标签页被切到后台时
// 倒计时仍保持实时。受 CSP（script-src 'self'）约束，这里不用任何
// 内联脚本；样式调整走 CSSOM（CSP 允许）。
(function () {
  "use strict";
  var examId = window.__hyzPipExamId;
  var startMs = Number(window.__hyzPipStartMs);
  var targetMs = Number(window.__hyzPipTargetMs);
  var totalSeconds = Number(window.__hyzPipTotalSeconds);
  var paused = Boolean(window.__hyzPipPaused);
  var pausedRemainingSeconds = Number(window.__hyzPipRemainingSeconds);
  var pausedProgressPercent = Number(window.__hyzPipProgressPercent);
  var isCustom =
    window.__hyzPipMode === "custom" ||
    (typeof examId === "string" && examId.indexOf("custom-") === 0);
  if (!examId || (!(targetMs > 0) && !(totalSeconds > 0))) {
    return;
  }
  var DAY_SECONDS = 24 * 60 * 60;
  var SOON_SECONDS = 120 * DAY_SECONDS;
  var URGENT_SECONDS = 45 * DAY_SECONDS;

  var card = document.querySelector('[data-exam-id="' + examId + '"]');
  if (!card) {
    return;
  }
  var counter = card.querySelector("[data-countdown-values]");
  var statusEl = card.querySelector("[data-status]");
  var progress = card.querySelector("progress");
  var progressMeta = card.querySelector("[data-progress-meta]");
  var titleEl = card.querySelector("h3");
  var valueEls = {};
  if (counter) {
    counter.querySelectorAll("strong[data-value]").forEach(function (el) {
      valueEls[el.getAttribute("data-value")] = el;
    });
  }
  var finishedShown = false;

  function pad(value) {
    return value < 10 ? "0" + value : String(value);
  }

  function applyTone(finished, remainingSeconds) {
    card.classList.remove("border-error/60", "border-warning/60");
    if (finished) {
      card.classList.add("border-base-content/10", "bg-base-300/60", "opacity-75");
    } else if (isCustom && paused) {
      card.classList.add("border-warning/60");
    } else if (remainingSeconds <= URGENT_SECONDS) {
      card.classList.add("border-error/60");
    } else if (remainingSeconds <= SOON_SECONDS) {
      card.classList.add("border-warning/60");
    }
  }

  function tick() {
    var now = Date.now();
    var configuredTotalMs = totalSeconds > 0 ? totalSeconds * 1000 : targetMs - startMs;
    var totalMs = Math.max(0, configuredTotalMs);
    var remainingMs = paused
      ? Math.max(0, pausedRemainingSeconds * 1000)
      : Math.max(0, targetMs - now);
    var remainingSeconds = paused
      ? Math.max(0, Math.ceil(pausedRemainingSeconds))
      : Math.ceil(remainingMs / 1000);
    var finished = paused ? remainingSeconds === 0 : now >= targetMs;
    var percent = paused
      ? Math.max(0, Math.min(100, Number.isFinite(pausedProgressPercent) ? pausedProgressPercent : 0))
      : totalMs === 0
        ? finished
          ? 100
          : 0
        : Math.min(100, Math.floor((Math.min(Math.max(0, now - startMs), totalMs) * 100) / totalMs));
    if (paused && totalMs > 0 && pausedProgressPercent !== percent) {
      pausedProgressPercent = percent;
    }
    var days = Math.floor(remainingSeconds / DAY_SECONDS);
    var hours = Math.floor((remainingSeconds % DAY_SECONDS) / 3600);
    var minutes = Math.floor((remainingSeconds % 3600) / 60);
    var seconds = remainingSeconds % 60;

    var label = isCustom
      ? finished
        ? "已结束"
        : paused
          ? "已暂停"
          : "计时中"
      : finished
        ? "已结束"
        : remainingSeconds <= URGENT_SECONDS
          ? "冲刺期"
          : remainingSeconds <= SOON_SECONDS
            ? "临近"
            : "备考中";
    if (statusEl && statusEl.textContent !== label) {
      statusEl.textContent = label;
    }

    if (finished) {
      if (!finishedShown && counter) {
        counter.innerHTML =
          '<strong class="text-2xl font-black leading-none text-base-content/65">' +
          (isCustom ? "时间到" : "考试日已过") +
          "</strong>";
        finishedShown = true;
      }
    } else if (valueEls.days) {
      valueEls.days.textContent = String(days);
      valueEls.hours.textContent = pad(hours);
      valueEls.minutes.textContent = pad(minutes);
      valueEls.seconds.textContent = pad(seconds);
    }

    if (progress) {
      progress.value = String(percent);
      progress.setAttribute(
        "aria-label",
        (titleEl ? titleEl.textContent : "") +
          (isCustom ? "倒计时进度 " : "冲刺进度 ") +
          percent +
          "%",
      );
    }
    if (progressMeta) {
      var metaSpans = progressMeta.querySelectorAll("span");
      if (metaSpans.length > 1) {
        metaSpans[1].textContent = percent + "%";
      }
    }
    if (card) {
      card.setAttribute("data-remaining-seconds", String(remainingSeconds));
      card.setAttribute(
        "aria-label",
        isCustom
          ? finished
            ? "自定义倒计时已结束"
            : paused
              ? "自定义倒计时已暂停，剩余 " + minutes + " 分 " + seconds + " 秒"
              : "自定义倒计时，剩余 " + days + " 天 " + hours + " 时 " + minutes + " 分 " + seconds + " 秒"
          : finished
            ? (titleEl ? titleEl.textContent : "") + "，考试已结束"
            : (titleEl ? titleEl.textContent : "") + "，距离考试 " + days + " 天",
      );
    }
    applyTone(finished, remainingSeconds);
  }

  // 画中画窗口通常较小，把数字放大到可读尺寸（CSSOM 变更不受 CSP style-src 限制）。
  if (counter) {
    counter.querySelectorAll("strong").forEach(function (el) {
      el.style.fontSize = "2.75rem";
      el.style.lineHeight = "1.1";
    });
    counter.querySelectorAll("span").forEach(function (el) {
      el.style.fontSize = "1rem";
    });
  }
  card.style.padding = "1.25rem";
  card.style.gap = "0.75rem";

  var closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.setAttribute("aria-label", "关闭画中画");
  closeButton.textContent = "×";
  closeButton.style.cssText =
    "position:fixed;top:0.5rem;right:0.5rem;z-index:20;display:grid;" +
    "width:2rem;height:2rem;place-items:center;border-radius:9999px;" +
    "border:1px solid rgba(248,248,242,0.25);background:rgba(40,42,54,0.92);" +
    "color:rgba(248,248,242,0.85);font-size:1.25rem;line-height:1;cursor:pointer;";
  closeButton.addEventListener("click", function () {
    window.close();
  });
  document.body.appendChild(closeButton);

  window.addEventListener("keydown", function (event) {
    if (event.key === "Escape" || event.code === "Escape") {
      window.close();
    }
  });

  tick();
  setInterval(tick, 1000);
})();
