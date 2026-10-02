"""Same-origin browser acceptance. Run only in the declared local/Cally sandbox."""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import time
import traceback
import sys

from playwright.sync_api import expect, sync_playwright

REPO = Path(__file__).resolve().parents[2]


@contextlib.contextmanager
def server(binary: Path, arguments: list[str]):
    process = subprocess.Popen([str(binary), *arguments], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            if not selector.select(12):
                raise AssertionError('The UI server did not report a launch URL within 12 seconds')
            line = process.stdout.readline().strip()
        match = re.fullmatch(r'(http://127\.0\.0\.1:\d+/)#token=([A-Za-z0-9_-]{43})', line)
        if not match:
            # Deliberately omit stdout: it may contain the launch secret.
            raise AssertionError('The UI server did not report the contracted launch URL')
        yield process, match[1], match[2]
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=3)
                raise AssertionError('The UI server exceeded its shutdown allowance')
        process.stdout.close()
        process.stderr.close()


def launch(page, origin, token):
    page.goto(origin + '#token=' + token, wait_until='networkidle')
    expect(page.get_by_role('heading', name='Workspace checks', exact=True)).to_be_visible()
    assert '#' not in page.url and token not in page.url, 'Launch fragment was not removed'
    assert page.evaluate('window.__WORKBENCH_BOOTSTRAP === undefined'), 'Token handoff was retained'


def screenshot(page, path):
    # Capture fixed navigation at its intended viewport position, after focus scrolling.
    page.evaluate('window.scrollTo(0, 0)')
    page.screenshot(path=str(path), full_page=True)


def downloaded_receipt(page, expected_bytes=None):
    with page.expect_download() as event:
        page.get_by_role('button', name='Download result').click()
    content = Path(event.value.path()).read_bytes()
    if expected_bytes is not None:
        assert content == expected_bytes, 'Downloaded receipt bytes differ from the API response'
    return json.loads(content)


def completed_check(page, button='Run check'):
    with page.expect_response(lambda response: response.request.method == 'GET'
                              and response.url.endswith('/receipt'), timeout=15_000) as response:
        page.get_by_role('button', name=button, exact=True).click()
    assert response.value.status == 200
    receipt = response.value.json()
    expect(page.get_by_label('Check result').locator('header').get_by_text(receipt['execution']['status'].replace('_', ' '), exact=True)).to_be_visible()
    return receipt


def numerical_flow(browser, binary, output, evidence):
    with server(binary, ['ui', 'serve', '--allow', 'task.run']) as (process, origin, token):
        context = browser.new_context(viewport={'width': 1440, 'height': 1100}, color_scheme='light')
        page = context.new_page()
        errors = []
        receipt_requests = []
        page.on('pageerror', lambda error: errors.append(type(error).__name__))
        page.on('request', lambda request: receipt_requests.append(request.url) if request.url.endswith('/receipt') else None)
        launch(page, origin, token)
        expect(page.get_by_role('heading', name='Your next result starts here')).to_be_visible()
        expect(page.get_by_label('Check', exact=True)).to_have_value('budget')
        # Inline validation is a real keyboard flow; no invocation should be admitted.
        page.get_by_label('Availability SLO (%)').fill('100')
        page.get_by_label('Availability SLO (%)').press('Tab')
        expect(page.get_by_text('Enter an SLO greater than 0 and less than 100%.')).to_be_visible()
        page.get_by_label('Availability SLO (%)').fill('99.9')
        page.get_by_label('Window (days)').fill('28')
        page.get_by_label('Bad minutes').fill('20')
        page.get_by_role('button', name='Calculate budget', exact=True).focus()
        page.keyboard.press('Enter')
        expect(page.get_by_role('heading', name='Budget remaining', exact=True)).to_be_visible(timeout=15_000)
        expect(page.get_by_text('40.32', exact=False).first).to_be_visible()
        expect(page.get_by_text('20.32', exact=False).first).to_be_visible()
        expect(page.get_by_label('Check result').get_by_text('succeeded', exact=True)).to_be_visible()
        assert 'run=' in page.url
        run_url = page.url
        receipt = downloaded_receipt(page)
        assert receipt['execution']['status'] == 'succeeded'
        assert receipt['assessment'] == 'not_assessed'
        assert receipt['record_mode'] == 'never'
        assert abs(receipt['data']['calculation']['status']['remaining'] - 20.32) < 1e-9
        assert len(receipt_requests) == 1, 'Fetching or downloading one selected result loaded extra receipts'
        screenshot(page, output / 'checks-light.png')
        page.get_by_role('button', name='Dark', exact=True).filter(visible=True).click()
        assert page.locator('html').get_attribute('data-theme') == 'dark'
        screenshot(page, output / 'checks-dark.png')
        page.get_by_role('link', name='Details', exact=True).click()
        expect(page.get_by_role('heading', name='Scope and limitations')).to_be_visible()
        page.get_by_role('link', name='Run history').filter(visible=True).click()
        expect(page.get_by_role('heading', name='Run history', exact=True)).to_be_visible()
        assert len(receipt_requests) == 1, 'History fetched full receipts instead of summaries'
        screenshot(page, output / 'history-dark.png')
        expect(page.get_by_label('Session history').get_by_role('button', name=re.compile('Error budget'))).to_have_count(1)
        page.get_by_label('Session history').get_by_role('button', name=re.compile('Error budget')).click()
        expect(page.get_by_role('heading', name='Budget remaining', exact=True)).to_be_visible()
        assert len(receipt_requests) == 2, 'The deselected receipt was retained in the client cache'
        assert page.url == run_url
        storage = page.evaluate('({local: {...localStorage}, session: {...sessionStorage}})')
        assert storage == {'local': {'agenticsre.theme': 'dark'}, 'session': {}}
        assert not context.cookies()
        page.reload(wait_until='networkidle')
        expect(page.get_by_role('heading', name='Reopen the workbench launch link')).to_be_visible()
        # Reopening the actual launch URL restores access to server-side history.
        launch(page, origin, token)
        page.get_by_role('link', name='Run history').filter(visible=True).click()
        expect(page.get_by_label('Session history').get_by_role('button', name=re.compile('Error budget'))).to_have_count(1)
        page.get_by_role('button', name='Clear history', exact=True).click()
        page.get_by_role('button', name='Clear completed results', exact=True).click()
        expect(page.get_by_role('heading', name='No checks in this session')).to_be_visible()
        # A native drawer must support keyboard dismissal and fit a narrow display.
        page.set_viewport_size({'width': 390, 'height': 844})
        page.get_by_role('button', name='Open navigation').click()
        expect(page.get_by_role('dialog', name='Navigation')).to_be_visible()
        page.keyboard.press('Escape')
        expect(page.get_by_role('dialog', name='Navigation')).not_to_be_visible()
        page.get_by_role('link', name='Go to workbench').click()
        screenshot(page, output / 'checks-mobile-dark.png')
        assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth'), 'Mobile view overflows horizontally'
        assert not errors, 'Browser emitted an application exception'
        evidence.append({'flow': 'numerical, keyboard, receipt download, themes, history, clear, bootstrap, mobile', 'status': 'passed'})
        # Refuse to create a fresh submission after a transport failure.
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=8)
        page.get_by_role('button', name='Calculate budget', exact=True).click()
        expect(page.get_by_role('heading', name='The submission outcome is unknown')).to_be_visible(timeout=12_000)
        expect(page.get_by_role('button', name='Calculate budget', exact=True)).to_be_disabled()
        screenshot(page, output / 'unavailable-mobile-dark.png')
        evidence.append({'flow': 'server disconnect, uncertain admission blocks a new submission', 'status': 'passed'})
        context.close()


def lost_response_flow(browser, binary, evidence):
    with server(binary, ['ui', 'serve', '--allow', 'task.run']) as (_, origin, token):
        context = browser.new_context()
        page = context.new_page()
        submissions = []
        accepted = []

        def lose_first_reply(route):
            if route.request.method != 'POST':
                route.continue_()
                return
            submissions.append(route.request.post_data_json)
            if len(submissions) == 1:
                # A real admission happens; only its delivery to the browser is lost.
                response = route.fetch()
                assert response.status == 202
                accepted.append(response.json()['id'])
                route.abort('connectionreset')
            else:
                route.continue_()

        page.route('**/api/v1/runs', lose_first_reply)
        launch(page, origin, token)
        page.get_by_role('button', name='Calculate budget').click()
        expect(page.get_by_role('heading', name='The submission outcome is unknown')).to_be_visible()
        assert len(submissions) == 1, 'Admission was automatically retried'
        expect(page.get_by_role('button', name='Calculate budget')).to_be_disabled()
        page.get_by_role('button', name='Reconcile submission').click()
        expect(page.get_by_role('heading', name='Budget remaining')).to_be_visible(timeout=15_000)
        assert len(submissions) == 2 and submissions[0] == submissions[1], 'Reconciliation changed the submission identity or payload'
        assert accepted[0] in page.url
        page.get_by_role('link', name='Run history').filter(visible=True).click()
        expect(page.get_by_label('Session history').get_by_role('button', name=re.compile('Error budget'))).to_have_count(1)
        evidence.append({'flow': 'lost real admission reply, explicit same-ID reconciliation, one execution in history', 'status': 'passed'})
        context.close()


def no_grants_flow(browser, binary, output, evidence):
    with server(binary, ['ui', 'serve']) as (_, origin, token):
        context = browser.new_context(viewport={'width': 1280, 'height': 900})
        page = context.new_page()
        launch(page, origin, token)
        expect(page.get_by_role('heading', name='No checks are granted')).to_be_visible()
        expect(page.get_by_role('button', name='Run check', exact=True)).to_have_count(0)
        expect(page.get_by_role('button', name='Calculate budget', exact=True)).to_have_count(0)
        screenshot(page, output / 'no-grants-light.png')
        page.goto(origin + '#token=' + 'a' * 43, wait_until='networkidle')
        expect(page.get_by_role('heading', name='Reopen the workbench launch link')).to_be_visible()
        evidence.append({'flow': 'real empty grants and invalid bearer, no runnable controls', 'status': 'passed'})
        context.close()


@contextlib.contextmanager
def checkout_fixture():
    """Use the same three trusted bindings/policy shape as the native profile lane."""
    bindings = {}
    for name in ('git', 'rg', 'bwrap'):
        value = os.environ.get('WORKBENCH_PROFILE_' + name.upper())
        if not value:
            raise AssertionError('Required native browser mode is missing a trusted ' + name + ' fixture binding')
        executable = Path(value).resolve(strict=True)
        with executable.open('rb') as handle:
            digest = hashlib.file_digest(handle, 'sha256').hexdigest()
        bindings[name] = {'path': str(executable), 'sha256': 'sha256:' + digest}
    environment = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8',
                   'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_SYSTEM': '/dev/null',
                   'GIT_CONFIG_NOSYSTEM': '1', 'GIT_TERMINAL_PROMPT': '0'}
    with tempfile.TemporaryDirectory(prefix='workbench-ui-fixture-') as temporary:
        scratch = Path(temporary)
        checkout = scratch / 'checkout'
        checkout.mkdir()

        def git(*args):
            subprocess.run([bindings['git']['path'], '-c', 'core.hooksPath=/nonexistent',
                            '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@example.invalid', *args],
                           cwd=checkout, env=environment, check=True, timeout=10,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

        git('init', '--template=', '--object-format=sha1', '--initial-branch=fixture')
        (checkout / 'tracked.txt').write_text('original\n')
        (checkout / 'literal λ.txt').write_text('café\n')
        git('add', '--', 'tracked.txt', 'literal λ.txt')
        git('commit', '-m', 'Browser fixture initial')
        (checkout / 'tracked.txt').write_text('needle <script>window.__output_executed=1</script> https://example.invalid\x1b[31m\u202e\u2066\n')
        os.mkfifo(checkout / 'blocked.fifo', 0o600)
        policy = scratch / 'policy.json'
        policy.write_text(json.dumps({'version': 1, 'profile': 'linux-read-v1',
                                     'roots': [{'id': 'checkout', 'path': str(checkout)}],
                                     'executables': bindings}))
        policy.chmod(0o600)
        index_digest = hashlib.sha256((checkout / '.git/index').read_bytes()).hexdigest()
        yield checkout, policy
        assert hashlib.sha256((checkout / '.git/index').read_bytes()).hexdigest() == index_digest


def native_commands_flow(browser, binary, output, evidence):
    with checkout_fixture() as (checkout, policy), server(binary, [
        '--read-policy', str(policy), 'ui', 'serve', '--allow', 'process.exec',
        '--allow', 'command.inspect', '--root', 'checkout',
    ]) as (_, origin, token):
        context = browser.new_context(viewport={'width': 1440, 'height': 1100})
        page = context.new_page()
        submissions = []
        external_requests = []
        def observe(request):
            if not request.url.startswith(origin):
                external_requests.append(request.resource_type)
            if request.method == 'POST' and request.url == origin + 'api/v1/runs':
                submissions.append(request.post_data_json)
        page.on('request', observe)
        launch(page, origin, token)
        for check, expected_text in [('status', ' M tracked.txt'), ('diff', '+needle'),
                                     ('log', 'Browser fixture initial'), ('files', 'literal λ.txt')]:
            page.get_by_label('Check', exact=True).select_option(check)
            receipt = completed_check(page)
            assert receipt['execution']['status'] == 'succeeded'
            assert receipt['data']['isolation']['state'] == 'confirmed'
            assert receipt['data']['tool']['execution_confirmed'] is True
            assert expected_text in receipt['output']['stdout']
            expect(page.get_by_label('Check result').locator('pre').first).to_contain_text(expected_text.strip())
        page.get_by_label('Check', exact=True).select_option('search')
        page.get_by_label('Literal text').fill('needle')
        page.get_by_label('Path (optional)').fill('tracked.txt')
        receipt = completed_check(page)
        assert receipt['execution']['status'] == 'succeeded'
        output_block = page.get_by_label('Check result').locator('pre').first
        expect(output_block).to_contain_text('<script>window.__output_executed=1</script>')
        expect(output_block).to_contain_text(r'\u001b[31m')
        assert output_block.locator('a, img, script, iframe').count() == 0
        assert page.evaluate('window.__output_executed === undefined')
        assert not external_requests, 'Output triggered an external browser request'
        screenshot(page, output / 'native-search-light.png')
        page.get_by_role('link', name='Details', exact=True).click()
        page.get_by_text('Complete result JSON', exact=True).click()
        raw_view = page.get_by_label('Check result').locator('.raw-details pre')
        expect(raw_view).to_contain_text(r'\u202e\u2066')
        assert '\u202e' not in raw_view.inner_text() and '\u2066' not in raw_view.inner_text()
        assert downloaded_receipt(page) == receipt
        page.get_by_role('link', name='Output', exact=True).click()
        # Client-side refusal must not send an invocation at all.
        before = len(submissions)
        page.get_by_label('Path (optional)').fill('./.git/config')
        page.get_by_role('button', name='Run check', exact=True).click()
        expect(page.get_by_text('Use a relative path inside this workspace, without parent segments or Git metadata.')).to_be_visible()
        assert len(submissions) == before
        page.get_by_label('Path (optional)').fill('literal λ.txt')
        page.get_by_label('Literal text').fill('café')
        literal = completed_check(page)
        assert literal['execution']['status'] == 'succeeded'
        assert submissions[-1]['inputs']['args'] == ['-F', '-e', 'café', '--', 'literal λ.txt']
        page.get_by_label('Literal text').fill('no-such-pattern')
        no_match = completed_check(page)
        assert no_match['execution']['child_exit_code'] == 1
        assert no_match['execution']['status'] == 'failed'
        assert no_match['data']['tool']['execution_confirmed'] is True
        inspected = completed_check(page, 'Inspect')
        assert inspected['operation'] == 'command.inspect'
        assert inspected['data']['execution_performed'] is False
        expect(page.get_by_text(re.compile('Inspection only. The tool was not executed.'))).to_be_visible()
        # Existing rg read-only FIFO behavior supplies an ordinary real long-running check.
        page.get_by_label('Path (optional)').fill('blocked.fifo')
        page.get_by_label('Literal text').fill('needle')
        page.get_by_role('button', name='Run check', exact=True).click()
        expect(page.get_by_role('heading', name='Check running', exact=True)).to_be_visible()
        expect(page.get_by_role('button', name='Run check', exact=True)).to_be_disabled()
        page.get_by_role('link', name='Run history').filter(visible=True).click()
        page.get_by_role('button', name='Clear history').click()
        page.get_by_role('button', name='Clear completed results', exact=True).click()
        expect(page.get_by_label('Session history').get_by_role('button', name=re.compile('Search text'))).to_have_count(1)
        page.get_by_label('Session history').get_by_role('button', name=re.compile('Search text')).click()
        with page.expect_response(lambda response: response.url.endswith('/receipt'), timeout=10_000) as response:
            page.get_by_role('button', name='Cancel check', exact=True).click()
        cancelled = response.value.json()
        assert cancelled['execution']['status'] == 'cancelled'
        expect(page.get_by_label('Check result').locator('header').get_by_text('cancelled', exact=True)).to_be_visible()
        assert downloaded_receipt(page, response.value.body()) == cancelled
        # Timeout is a distinct terminal result with the same honest cleanup path.
        page.get_by_label('Path (optional)').fill('blocked.fifo')
        page.get_by_label('Literal text').fill('needle')
        page.get_by_text('Run limits', exact=True).click()
        page.get_by_label('Timeout (seconds)').fill('1')
        started = time.monotonic()
        timed_out = completed_check(page)
        assert timed_out['execution']['status'] == 'timed_out'
        assert timed_out['data']['launcher']['state'] == 'tool_started'
        assert timed_out['data']['execution_performed'] is True
        # Termination may prevent the launcher's completion record; do not claim tool completion.
        assert timed_out['data']['tool']['execution_confirmed'] is False
        assert time.monotonic() - started < 5
        assert timed_out['output']['stdout'] == ''
        assert timed_out['output']['stderr'] == ''
        assert not (checkout / 'marker').exists()
        evidence.append({'flow': 'native Git status/diff/log, rg list/literal/nonzero, inspect, metadata refusal, text-only output, cancel, clear preserves active, deadline', 'status': 'passed'})
        context.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=REPO / 'target/debug/save')
    args = parser.parse_args()
    executable = os.environ.get('WORKBENCH_BROWSER_EXECUTABLE')
    if not executable:
        raise SystemExit('WORKBENCH_BROWSER_EXECUTABLE must select the staged browser in the declared sandbox')
    output = Path(os.environ.get('WORKBENCH_UI_OUTPUT', REPO / 'target/ui'))
    output.mkdir(parents=True, exist_ok=True)
    evidence = []
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=executable, headless=True,
                                             args=['--no-sandbox', '--disable-dev-shm-usage'])
        try:
            numerical_flow(browser, args.binary.resolve(), output, evidence)
            lost_response_flow(browser, args.binary.resolve(), evidence)
            no_grants_flow(browser, args.binary.resolve(), output, evidence)
            if os.environ.get('WORKBENCH_PROFILE_REQUIRED') == '1':
                native_commands_flow(browser, args.binary.resolve(), output, evidence)
            else:
                evidence.append({'flow': 'native restricted commands', 'status': 'not_run',
                                 'reason': 'Requires the explicit Cally profile boundary and trusted fixture bindings'})
        finally:
            browser.close()
    (output / 'browser-results.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps({'status': 'passed',
                      'passed_flows': sum(item['status'] == 'passed' for item in evidence),
                      'not_run_flows': sum(item['status'] == 'not_run' for item in evidence),
                      'evidence': str(output / 'browser-results.json')}))


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # Even assertion diagnostics must not retain a launch fragment.
        sys.stderr.write(re.sub(r'#token=[A-Za-z0-9_-]+', '#token=[redacted]', traceback.format_exc()))
        raise SystemExit(1) from None
