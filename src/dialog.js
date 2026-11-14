// keymule's dialogs on macOS: a small window showing src/dialog.html,
// run by `osascript -l JavaScript`.
//
// The spec arrives as a JSON literal in the script keymule pipes to
// osascript's stdin, and the page builds its text with textContent, so names
// and URLs chosen by an agent can't inject code. Prints
// {"button": <label>, "text": <typed text, secure field only>}; throws
// "keymule-timeout" when nobody answers in time.
//
// spec: {
//   title, body,          // headline, and the text below it; lines indented
//                         // by 8 spaces show as code
//   icon, tone,           // "key" | "unlock" | "trash" | "eye" | "shield";
//                         // "" (accent) | "warn" | "bad"
//   buttons: [labels],    // left to right
//   primary,              // the filled button
//   default, cancel,      // labels: Return and Escape
//   destructive,          // label drawn as a red button
//   secure,               // true: a hidden text field
//   timeout               // seconds
// }

ObjC.import('AppKit');
ObjC.import('WebKit');

const WIDTH = 440;
let answer = null;
let height = 0;

ObjC.registerSubclass({
  name: 'KeymuleDialogHandler',
  methods: {
    'userContentController:didReceiveScriptMessage:': {
      types: ['void', ['id', 'id']],
      implementation: function (_controller, message) {
        const msg = JSON.parse(ObjC.unwrap(message.body));
        if (msg.height) {
          height = msg.height;
        } else if (answer === null) {
          answer = msg;
          $.NSApp.stopModal;
        }
      },
    },
  },
});

function show(spec, html) {
  const app = $.NSApplication.sharedApplication;
  app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);

  const config = $.WKWebViewConfiguration.alloc.init;
  config.userContentController.addScriptMessageHandlerName(
    $.KeymuleDialogHandler.alloc.init, 'keymule');
  const frame = $.NSMakeRect(0, 0, WIDTH, 200);
  const web = $.WKWebView.alloc.initWithFrameConfiguration(frame, config);
  web.setValueForKey(false, 'drawsBackground');

  // Titled (so it can take keyboard focus) but with the title bar hidden.
  const style = $.NSWindowStyleMaskTitled | $.NSWindowStyleMaskFullSizeContentView;
  const win = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer(
    frame, style, $.NSBackingStoreBuffered, false);
  win.titlebarAppearsTransparent = true;
  win.titleVisibility = $.NSWindowTitleHidden;
  win.movableByWindowBackground = true;
  win.releasedWhenClosed = false;
  win.level = $.NSFloatingWindowLevel;
  win.contentView = web;

  const json = JSON.stringify(spec).replace(/</g, '\\u003c');
  const page = html.replace('/*SPEC*/null', () => json);
  web.loadHTMLStringBaseURL(page, $());

  // Wait for the page to say how tall it is, then size and show the window.
  const until = $.NSDate.dateWithTimeIntervalSinceNow(5);
  while (!height && $.NSDate.date.compare(until) < 0) {
    $.NSRunLoop.currentRunLoop.runModeBeforeDate(
      $.NSDefaultRunLoopMode, $.NSDate.dateWithTimeIntervalSinceNow(0.02));
  }
  if (!height) throw new Error('the dialog page did not load');
  win.setContentSize($.NSMakeSize(WIDTH, height));
  win.center;
  app.activateIgnoringOtherApps(true);
  win.makeKeyAndOrderFront($());
  win.makeFirstResponder(web);

  if (spec.timeout) {
    app.performSelectorWithObjectAfterDelayInModes(
      'abortModal', $(), spec.timeout, $([$.NSModalPanelRunLoopMode]));
  }
  app.runModalForWindow(win);
  win.orderOut($());
  if (answer === null) throw new Error('keymule-timeout');
  return JSON.stringify(answer);
}
