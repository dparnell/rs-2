#!/bin/sh
# WebRS2 (WEB.EXE) against the test site in tools/webtest (docs/BROWSER.md):
# serves it on the host's port 8471, opens it in WEB under QEMU and drives
# it: a click on the "Second" link, Alt+Left back, Tab Tab Enter, back, a
# word typed into the form and Enter; then the JavaScript page. Screenshots
# work/webtest1.png ... webtest7.png; the host's request log should end with
# "GET /second.html?q=hello" and js.html, lib.js.
R=$(cd "$(dirname "$0")/../.." && pwd)
D=$R/work/webtest
rm -rf "$D"; mkdir -p "$D/www"
cp "$R"/tools/webtest/*.html "$R"/tools/webtest/*.js "$D/www/"
python3 -I "$R/tools/webtest/grad.py" "$D/www/grad.png"
(cd "$D/www" && exec python3 -I -m http.server 8471 --bind 127.0.0.1) > "$D/server.log" 2>&1 &
srv=$!
cd "$R"
# Screen positions (1024 x 768, absolute pointer 0..32767): the "Second"
# link in the header, the form's text field.
cargo xtask run --headless --net --timeout 240 --init 'OS2\APPS\WEB.EXE -retry http://10.0.2.2:8471/' \
  --type-at 75 --type '{shot webtest1}{mouse_abs 27231 5248}{mouse_button 1}{mouse_button 0}{sleep 15}{shot webtest2}{alt-left}{sleep 15}{shot webtest3}{tab}{tab}{sleep 3}{shot webtest4}{ret}{sleep 15}{alt-left}{sleep 15}{mouse_abs 6496 25386}{mouse_button 1}{mouse_button 0}{sleep 3}hello
{sleep 15}{shot webtest5}{ctrl-l}{sleep 1}http://10.0.2.2:8471/js.html
{sleep 20}{shot webtest6}' \
  --screenshot work/webtest7.png > "$D/guest.log" 2>&1
kill $srv
grep -a '"GET' "$D/server.log"
