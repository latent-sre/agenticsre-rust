"""Same-origin browser acceptance. Run only in the declared local/Cally sandbox."""
from __future__ import annotations

import argparse
import base64
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import selectors
import signal
import shutil
import subprocess
import tempfile
import time
import traceback
import sys

from playwright.sync_api import expect, sync_playwright

REPO = Path(__file__).resolve().parents[2]


@contextlib.contextmanager
def server(binary: Path, arguments: list[str], *, cwd: Path | None = None):
    process = subprocess.Popen([str(binary), *arguments], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, start_new_session=True, cwd=cwd)
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


def diagnostics_flow(browser, binary, output, evidence):
    """Run the real installed five-check pack; retain original bytes for schema verification."""
    specification = importlib.util.spec_from_file_location('browser_diagnostic_fixtures', REPO / 'tools/verify-diagnostics.py')
    fixture = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(fixture)
    records = []
    with tempfile.TemporaryDirectory(prefix='workbench-browser-diagnostics-') as temporary:
        directory = Path(temporary)
        installed = directory / 'installed/bin/save'
        installed.parent.mkdir(parents=True)
        shutil.copyfile(binary, installed)
        installed.chmod(0o755)
        binary_sha = hashlib.sha256(binary.read_bytes()).hexdigest()
        assert hashlib.sha256(installed.read_bytes()).hexdigest() == binary_sha
        certs = fixture.certificates(directory)
        dns = fixture.Dns()
        https = fixture.Https(certs['server.pem'], certs['server.key'])
        host = 'diagnostics.workbench.test'
        alias = 'alias.workbench.test'
        dns.answers[host, 1] = [(host, 5, alias), (alias, 1, '127.0.0.1')]
        target = {'id': 'fixture-web', 'host': host, 'port': https.port, 'path': '/probe',
                  'allowed_ips': ['127.0.0.1'],
                  'resolver': {'kind': 'explicit', 'address': '127.0.0.1', 'port': dns.port, 'transport': 'udp'},
                  'tls_ca_file': str(certs['ca.pem']), 'process': fixture.process_identity(os.getpid())}
        config = directory / 'config.json'
        config.write_text(json.dumps({'schema_version': 1, 'targets': [target, {**target, 'id': 'fixture-other'}]}))
        context = browser.new_context(viewport={'width': 1440, 'height': 1100}, color_scheme='light')
        page = context.new_page()
        errors, external_requests, submissions = [], [], []
        page.on('pageerror', lambda error: errors.append(type(error).__name__))

        def keep_receipt(label, content):
            path = output / ('diagnostics-' + label + '.json')
            path.write_bytes(content)
            value = json.loads(content)
            assert value['assessment'] == 'not_assessed' and value['record_mode'] == 'never'
            assert value['execution']['child_exit_code'] is None and value['execution']['signal'] is None
            assert [step['id'] for step in value['data']['steps']] == ['dns', 'tcp', 'tls', 'http', 'process']
            records.append({'label': label, 'file': path.name, 'sha256': hashlib.sha256(content).hexdigest()})
            return value

        def finished(label, action):
            with page.expect_response(lambda response: response.request.method == 'GET'
                                      and response.url.endswith('/receipt'), timeout=15_000) as response:
                action()
            assert response.value.status == 200
            original = response.value.body()
            value = keep_receipt(label, original)
            expect(page.get_by_label('Check result').locator('.result-heading .badge')).to_have_text(value['execution']['status'].replace('_', ' '))
            return value, original

        def cli_parity(label, value, request):
            inputs = directory / 'input.json'
            inputs.write_text(json.dumps(request['inputs']['input']))
            limits = request['limits']
            env = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8', 'TZ': 'UTC'}
            command = [str(installed), '--json', '--diagnostics-config', str(config), 'diagnose',
                       '--target', request['target_id'], '--pack', 'connectivity', '--input', str(inputs),
                       '--timeout', str(limits['timeout_ms']) + 'ms', '--max-output-bytes', str(limits['max_output_bytes']),
                       '--concurrency', str(limits['concurrency'])]
            execution = subprocess.run(command, cwd=directory, env=env, capture_output=True, timeout=12)
            assert execution.returncode == (0 if value['execution']['status'] == 'succeeded' else 1), 'CLI parity invocation exit'
            assert not execution.stderr, 'CLI parity emitted diagnostics'
            direct = keep_receipt(label + '-cli', execution.stdout)
            assert fixture.comparable(value) == fixture.comparable(direct), 'Browser/CLI parity changed a nonvolatile observation'
            wire = directory / 'request.json'
            wire.write_text(json.dumps({'spec_version': '0.1', 'request_id': 'browser-core-parity',
                                       'operation': 'diagnostics.run', 'operation_version': 1,
                                       'target': {'kind': 'connection', 'id': request['target_id']},
                                       'inputs': request['inputs'], 'limits': limits, 'record': 'never'}))
            execution = subprocess.run([str(installed), '--json', '--diagnostics-config', str(config),
                                        'call', '--request', str(wire)], cwd=directory, env=env, capture_output=True, timeout=12)
            assert execution.returncode == (0 if value['execution']['status'] == 'succeeded' else 1) and not execution.stderr
            called = keep_receipt(label + '-call', execution.stdout)
            assert fixture.comparable(value) == fixture.comparable(called), 'Browser/call parity changed a nonvolatile observation'

        try:
            with server(installed, ['--diagnostics-config', str(config), 'ui', 'serve',
                                    '--allow', 'diagnostics.run', '--target', 'fixture-web'], cwd=directory) as (_, origin, token):
                def observe(request):
                    if not request.url.startswith(origin):
                        external_requests.append(request.resource_type)
                    if request.method == 'POST' and request.url == origin + 'api/v1/runs':
                        submissions.append(request.post_data_json)
                page.on('request', observe)
                launch(page, origin, token)
                expect(page.get_by_label('Check', exact=True)).to_have_value('diagnostics')
                expect(page.get_by_label('Diagnostic target')).to_have_value('fixture-web')
                expect(page.get_by_label('Diagnostic target').locator('option')).to_have_count(1)
                expect(page.get_by_role('option', name='fixture-other')).to_have_count(0)
                expect(page.get_by_label('Capture response body')).not_to_be_checked()
                expect(page.get_by_label('Workspace', exact=True)).to_have_count(0)
                expect(page.get_by_role('button', name='Inspect', exact=True)).to_have_count(0)
                # Reach the primary action only through the native form's tab order.
                page.get_by_label('Check', exact=True).focus()
                for identity in ('target', 'onFailure', 'concurrency', 'captureBody'):
                    page.keyboard.press('Tab')
                    assert page.evaluate('document.activeElement.id') == identity
                page.keyboard.press('Tab')  # Native Run limits disclosure.
                assert page.evaluate('document.activeElement.tagName') == 'SUMMARY'
                page.keyboard.press('Tab')
                assert page.evaluate('document.activeElement.textContent').strip().startswith('Run diagnostics')
                mission, original = finished('browser-mission', lambda: page.keyboard.press('Enter'))
                assert all(step['attempted'] and step['state'] == 'succeeded' for step in mission['data']['steps'])
                assert mission['coverage']['state'] == 'complete'
                assert mission['data']['steps'][3]['observation']['body_capture']['content'] is None
                assert mission['data']['steps'][4]['observation']['pid'] == os.getpid()
                assert mission['data']['steps'][0]['observation']['cname_chains']['a'] == [host, alias]
                assert mission['data']['steps'][2]['observation']['hostname'] == host
                assert submissions[-1]['inputs']['input'] == {'capture_body': False, 'on_failure': 'continue'}
                assert 'root_id' not in submissions[-1]
                assert downloaded_receipt(page, original) == mission
                run_url = page.url
                dns_view = page.get_by_label('DNS lookup', exact=True)
                dns_view.locator('summary').click()
                expect(dns_view.get_by_text(host + ' → ' + alias, exact=True)).to_be_visible()
                expect(dns_view.get_by_text('DNS transport requests', exact=True)).to_be_visible()
                expect(dns_view.get_by_text('Requests handed to the transport. A closed stream can reject one before transmission.', exact=True)).to_be_visible()
                screenshot(page, output / 'diagnostics-light.png')
                page.get_by_role('button', name='Dark', exact=True).filter(visible=True).click()
                screenshot(page, output / 'diagnostics-dark.png')
                page.get_by_role('link', name='Details', exact=True).click()
                expect(page.get_by_role('heading', name='Diagnostic provenance')).to_be_visible()
                identity = mission['data']['binding']['identity']
                assert identity['configuration_sha256'] == 'sha256:' + hashlib.sha256(config.read_bytes()).hexdigest()
                assert identity['additional_ca_sha256'] == 'sha256:' + hashlib.sha256(certs['ca.pem'].read_bytes()).hexdigest()
                expect(page.get_by_text(mission['data']['pack']['sha256'], exact=True)).to_be_visible()
                screenshot(page, output / 'diagnostics-provenance-dark.png')
                page.get_by_role('link', name='Run history').filter(visible=True).click()
                history = page.get_by_label('Session history')
                expect(history.get_by_role('button', name=re.compile('Connectivity diagnostics'))).to_have_count(1)
                expect(history.get_by_text(re.compile('fixture-web'))).to_be_visible()
                history.get_by_role('button', name=re.compile('Connectivity diagnostics')).click()
                expect(page.get_by_role('heading', name='DNS lookup', exact=True)).to_be_visible()
                assert page.url == run_url
                assert downloaded_receipt(page, original) == mission
                cli_parity('mission', mission, submissions[-1])

                # Lose a real admission reply, then reconcile the identical diagnostic intent.
                reconciliations, accepted = [], []
                def lose_first_reply(route):
                    if route.request.method != 'POST':
                        route.continue_()
                        return
                    reconciliations.append(route.request.post_data_json)
                    if len(reconciliations) == 1:
                        response = route.fetch()
                        assert response.status == 202
                        accepted.append(response.json()['id'])
                        route.abort('connectionreset')
                    else:
                        route.continue_()
                before_dns, before_connections = len(dns.events), https.connections
                page.route('**/api/v1/runs', lose_first_reply)
                page.get_by_role('button', name='Run diagnostics', exact=True).click()
                expect(page.get_by_role('heading', name='The submission outcome is unknown')).to_be_visible()
                expect(page.get_by_role('button', name='Run diagnostics', exact=True)).to_be_disabled()
                assert len(reconciliations) == 1
                reconciled, _ = finished('browser-reconciled', lambda: page.get_by_role('button', name='Reconcile submission').click())
                assert reconciliations[0] == reconciliations[1] and accepted[0] in page.url
                assert len(dns.events) == before_dns + 2 and https.connections == before_connections + 3
                assert reconciled['execution']['status'] == 'succeeded'
                page.unroute('**/api/v1/runs', lose_first_reply)

                dns.answers.clear()
                dns.addresses = ['127.0.0.2']
                before_connections = https.connections
                partial, raw_partial = finished('browser-partial', lambda: page.get_by_role('button', name='Run diagnostics', exact=True).click())
                assert partial['execution']['status'] == 'partial'
                assert partial['data']['steps'][0]['reason'] == 'dns_address_denied'
                assert partial['data']['steps'][4]['state'] == 'succeeded'
                assert https.connections == before_connections
                for name in ('TCP connection', 'TLS validation', 'HTTP response'):
                    step = page.get_by_label(name, exact=True)
                    expect(step.get_by_text('Not run · Elapsed: Not recorded', exact=True)).to_be_visible()
                    expect(step.get_by_text('prerequisite_failed', exact=True)).to_be_visible()
                assert downloaded_receipt(page, raw_partial) == partial
                screenshot(page, output / 'diagnostics-partial-dark.png')
                cli_parity('partial', partial, submissions[-1])
                page.get_by_label('On a failed check').select_option('stop')
                page.get_by_label('Concurrent checks').select_option('1')
                stopped, _ = finished('browser-stop-serial', lambda: page.get_by_role('button', name='Run diagnostics', exact=True).click())
                assert stopped['data']['steps'][4]['attempted'] is False
                assert stopped['data']['steps'][4]['reason'] == 'stopped_after_failure'
                expect(page.get_by_label('Local process', exact=True).get_by_text('Not run · Elapsed: Not recorded', exact=True)).to_be_visible()
                cli_parity('stop-serial', stopped, submissions[-1])

                # Response-driven markup is always inert; body bytes remain lossless base64.
                dns.addresses = ['127.0.0.1']
                hostile = ('<script>window.__diagnostic_output_executed=1</script>'
                           '<img src="https://example.invalid"> https://example.invalid \x1b[31m\u202e\u2066\n').encode() + b'x' * 1200
                https.response = (b'HTTP/1.1 200 OK\r\nContent-Type: text/plain; note="<script>window.__diagnostic_output_executed=1</script>"\r\nContent-Length: '
                                  + str(len(hostile)).encode() + b'\r\n\r\n' + hostile)
                page.get_by_label('On a failed check').select_option('continue')
                page.get_by_label('Concurrent checks').select_option('2')
                page.get_by_label('Capture response body').check()
                page.get_by_text('Run limits', exact=True).click()
                page.get_by_label('Output allowance (bytes)').fill('1024')
                captured, _ = finished('browser-hostile-body', lambda: page.get_by_role('button', name='Run diagnostics', exact=True).click())
                body = captured['data']['steps'][3]['observation']['body_capture']
                assert captured['execution']['status'] == 'partial' and body['truncated'] and body['captured_bytes'] == 1024
                assert base64.b64decode(body['content']) == hostile[:1024]
                assert body['classification'] == 'untrusted_content' and not body['redaction_applied'] and not body['decompressed']
                http_view = page.get_by_label('HTTP response', exact=True)
                http_view.locator('summary').click()
                expect(http_view.get_by_text('Captured body (untrusted base64)', exact=True)).to_be_visible()
                expect(http_view.locator('.captured-body')).to_have_text(body['content'])
                expect(http_view.get_by_text('text/plain; note="<script>window.__diagnostic_output_executed=1</script>"', exact=True)).to_be_visible()
                assert http_view.locator('a, img, script, iframe').count() == 0
                assert page.evaluate('window.__diagnostic_output_executed === undefined')
                assert not external_requests, 'Diagnostic output caused an external browser request'
                screenshot(page, output / 'diagnostics-hostile-body-dark.png')
                cli_parity('hostile-body', captured, submissions[-1])

                # Cancellation starts only after the private service witnessed the actual GET.
                https.delay = 30
                before_http = len(https.events)
                page.get_by_role('button', name='Run diagnostics', exact=True).click()
                expect(page.get_by_role('heading', name='Check running', exact=True)).to_be_visible()
                expect(page.get_by_role('button', name='Run diagnostics', exact=True)).to_be_disabled()
                deadline = time.monotonic() + 5
                while len(https.events) == before_http:
                    assert time.monotonic() < deadline, 'No actual HTTP GET before browser cancellation'
                    page.wait_for_timeout(10)
                before_dns, before_connections = len(dns.events), https.connections
                cancelled, raw_cancelled = finished('browser-cancelled', lambda: page.get_by_role('button', name='Cancel check', exact=True).click())
                assert cancelled['execution']['status'] == 'cancelled' and cancelled['data']['cancellation_signal'] == 2
                assert cancelled['data']['steps'][3]['attempted'] and cancelled['data']['steps'][3]['state'] == 'cancelled'
                assert cancelled['data']['steps'][4]['state'] == 'succeeded'
                assert len(dns.events) == before_dns and https.connections == before_connections and len(https.events) == before_http + 1
                assert downloaded_receipt(page, raw_cancelled) == cancelled
                page.set_viewport_size({'width': 390, 'height': 844})
                screenshot(page, output / 'diagnostics-cancelled-mobile-dark.png')
                assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth'), 'Diagnostic mobile view overflows'
                page.get_by_role('button', name='Open navigation').click()
                expect(page.get_by_role('dialog', name='Navigation')).to_be_visible()
                page.keyboard.press('Escape')
                expect(page.get_by_role('dialog', name='Navigation')).not_to_be_visible()
                assert not errors, 'Diagnostics rendered an application exception'
                storage = page.evaluate('({local: {...localStorage}, session: {...sessionStorage}})')
                assert storage == {'local': {'agenticsre.theme': 'dark'}, 'session': {}} and not context.cookies()
        finally:
            context.close()
            https.close()
            dns.close()
        (output / 'diagnostics-browser-receipts.json').write_text(json.dumps({
            'binary_sha256': binary_sha, 'records': records,
            'parity_exclusions': ['request_id', 'run_id', 'started_at', 'finished_at', 'duration_ms',
                                  'sources[*].observed_at', 'data.vantage.observer_pid',
                                  'data.steps[*].started_offset_ms', 'data.steps[*].duration_ms', 'data.steps[*].observed_at',
                                  'data.steps[*].observation.local_address', 'data.steps[*].observation.vantage.observer_pid',
                                  'data.steps[*].observation.connection.local_address'],
            'cleanup': 'UI process joined; DNS and HTTPS fixture threads/sockets closed and joined',
            'dns_transport_events': len(dns.events), 'service_connections': https.connections,
            'http_requests': len(https.events),
        }, indent=2) + '\n')
        evidence.append({'flow': 'installed browser five-check mission, original download/history, CLI/call parity, CNAME provenance, keyboard, themes/mobile, diagnostic reconciliation, partial/stop/serial, inert captured body, witnessed HTTP cancellation',
                         'status': 'passed', 'binary_sha256': binary_sha, 'receipt_records': len(records)})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=REPO / 'target/debug/save')
    parser.add_argument('--diagnostics-only', action='store_true', help='Exercise only the new real diagnostics browser matrix')
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
            if not args.diagnostics_only:
                numerical_flow(browser, args.binary.resolve(), output, evidence)
                lost_response_flow(browser, args.binary.resolve(), evidence)
                no_grants_flow(browser, args.binary.resolve(), output, evidence)
                if os.environ.get('WORKBENCH_PROFILE_REQUIRED') == '1':
                    native_commands_flow(browser, args.binary.resolve(), output, evidence)
                else:
                    evidence.append({'flow': 'native restricted commands', 'status': 'not_run',
                                     'reason': 'Requires the explicit Cally profile boundary and trusted fixture bindings'})
            diagnostics_flow(browser, args.binary.resolve(), output, evidence)
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
