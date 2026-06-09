// === Vercel Analytics ===
function initAnalytics() {
  window.va =
    window.va ||
    function () {
      (window.vaq = window.vaq || []).push(arguments);
    };

  var analyticsHosts = new Set(['rewind.day', 'www.rewind.day']);
  if (!analyticsHosts.has(window.location.hostname)) {
    return;
  }

  if (document.querySelector('script[data-va-insights]')) {
    return;
  }

  var script = document.createElement('script');
  script.defer = true;
  script.src = 'https://cdn.vercel-insights.com/v1/script.js';
  script.setAttribute('data-va-insights', 'true');
  document.head.appendChild(script);
}

// === Clipboard Copy ===
function initClipboardCopy() {
  var copySuccessIcon =
    '<svg class="copy-icon" width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><use href="#icon-check"></use></svg>';

  document.querySelectorAll('.copy-btn').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var text = btn.dataset.copy;
      navigator.clipboard
        .writeText(text)
        .then(function () {
          var original = btn.innerHTML;
          btn.innerHTML = copySuccessIcon;
          btn.classList.add('copied');
          setTimeout(function () {
            btn.innerHTML = original;
            btn.classList.remove('copied');
          }, 2000);
        })
        .catch(function () {
          var code = btn.parentElement.querySelector('.install-cmd');
          if (code) {
            var range = document.createRange();
            range.selectNodeContents(code);
            var sel = window.getSelection();
            sel.removeAllRanges();
            sel.addRange(range);
          }
        });
    });
  });
}

// === FAQ Accordion ===
function initFaqAccordion() {
  document.querySelectorAll('.faq-question').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var item = btn.parentElement;
      var answer = item.querySelector('.faq-answer');
      var isOpen = item.classList.contains('open');

      // Close all
      document.querySelectorAll('.faq-item.open').forEach(function (openItem) {
        openItem.classList.remove('open');
        openItem.querySelector('.faq-answer').style.maxHeight = null;
        openItem.querySelector('.faq-question').setAttribute('aria-expanded', 'false');
      });

      // Toggle current
      if (!isOpen) {
        item.classList.add('open');
        answer.style.maxHeight = answer.scrollHeight + 'px';
        btn.setAttribute('aria-expanded', 'true');
      }
    });
  });
}

// === Scroll Fade-in ===
function initScrollFadeIn() {
  var elements = document.querySelectorAll('.fade-in');
  if (!elements.length) return;

  var observer = new IntersectionObserver(
    function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add('visible');
          observer.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.1 }
  );

  elements.forEach(function (el) {
    observer.observe(el);
  });
}

// === Init ===
document.addEventListener('DOMContentLoaded', function () {
  initAnalytics();
  initClipboardCopy();
  initFaqAccordion();
  initScrollFadeIn();
});
