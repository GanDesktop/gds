"""pytest owns collection, fixtures, assertions, selection and JUnit reporting."""

from datetime import datetime, timezone
import json
from pathlib import Path
import sys
import tarfile

import pytest

from driver import Blocked, Desktop, ROOT, Workflow, vm


def pytest_addoption(parser):
    group = parser.getgroup('GDS desktop VMs')
    group.addoption('--vm', default='all', help='Comma-separated machine aliases, or all Linux guests')
    group.addoption('--bundle', type=Path, default=ROOT / 'target/vm-e2e/build.tar.gz')
    group.addoption('--guest-bin-dir', help='Test already installed guest executables instead of a bundle')
    group.addoption('--artifacts', type=Path, help='Fresh artifact directory (defaults to target/vm-e2e/run-*)')


def pytest_configure(config):
    selection = config.getoption('--vm')
    names = [name for name in vm.MACHINES if name != 'freebsd'] if selection == 'all' else selection.split(',')
    if not names or any(name not in vm.MACHINES for name in names) or len(set(names)) != len(names):
        raise pytest.UsageError(f'--vm must select unique names from {list(vm.MACHINES)}')
    config.vm_names = names
    config.results = {}
    stamp = datetime.now(timezone.utc).strftime('run-%Y%m%dT%H%M%S-%fZ')
    config.artifacts = config.getoption('--artifacts') or ROOT / 'target/vm-e2e' / stamp
    config.artifacts.mkdir(parents=True, exist_ok=False)
    if not config.option.xmlpath:
        config.option.xmlpath = str(config.artifacts / 'junit.xml')
    (config.artifacts / 'invocation.json').write_text(json.dumps({
        'command': sys.argv, 'machines': names, 'bundle': str(config.getoption('--bundle')),
        'replay': 'just test-e2e --vm <machine> -k <case-id>',
    }, indent=2))
    with tarfile.open(config.artifacts / 'runner-source.tar.gz', 'w:gz') as archive:
        paths = [*Path(__file__).parent.glob('*.py'), *Path(__file__).parent.glob('*.toml'),
                 Path(__file__).with_name('uv.lock'), ROOT / 'scripts/vm.py', ROOT / 'scripts/e2e.py', ROOT / 'justfile']
        for path in paths:
            archive.add(path, arcname=str(path.relative_to(ROOT)))


def pytest_generate_tests(metafunc):
    if 'desktop' in metafunc.fixturenames:
        metafunc.parametrize('desktop', metafunc.config.vm_names, indirect=True, scope='session')


@pytest.fixture(scope='session')
def desktop(request):
    artifacts = request.config.artifacts / request.param
    artifacts.mkdir(exist_ok=True)
    session = None
    try:
        session = Desktop(request.param, artifacts, request.config.getoption('--bundle'),
                          request.config.getoption('--guest-bin-dir'))
    except Blocked as error:
        (artifacts / 'blocked.txt').write_text(str(error))
        pytest.skip(f'BLOCKED: {error}')
    try:
        yield session
    finally:
        session.close()


@pytest.fixture
def case(desktop, request):
    case_id = request.node.originalname.removeprefix('test_')
    directory = desktop.artifacts / case_id
    directory.mkdir()
    workflow = Workflow(desktop, case_id, directory)
    request.node.workflow = workflow
    try:
        yield workflow
    finally:
        try:
            workflow.checkpoint('final')
        finally:
            try:
                workflow.close()
            finally:
                workflow.collect()


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item, call):
    result = (yield).get_result()
    record = item.config.results.setdefault(item.nodeid, {'status': 'planned', 'seconds': 0})
    record['seconds'] += result.duration
    if result.failed:
        record.setdefault('failures', []).append({'phase': result.when, 'error': str(result.longrepr)})
        if record['status'] != 'failed':
            record.update(status='failed', phase=result.when, error=str(result.longrepr))
    elif result.skipped:
        record.update(status='blocked', phase=result.when, error=str(result.longrepr))
    elif result.when == 'call':
        record['call_passed'] = True
    elif result.when == 'teardown' and record['status'] == 'planned' and record.get('call_passed'):
        record['status'] = 'passed'
    if hasattr(item, 'workflow'):
        record.update(step=item.workflow.last_step, artifacts=str(item.workflow.artifacts))
    write_results(item.config)


def pytest_collection_finish(session):
    for item in session.items:
        session.config.results.setdefault(item.nodeid, {'status': 'planned', 'seconds': 0})
    write_results(session.config)


def write_results(config):
    temporary = config.artifacts / 'results.json.tmp'
    temporary.write_text(json.dumps(config.results, indent=2))
    temporary.replace(config.artifacts / 'results.json')
    rows = ['# VM E2E results', '', '| Workflow | Result | Seconds |', '| --- | --- | ---: |']
    rows.extend(f'| `{name}` | {result["status"].upper()} | {result["seconds"]:.2f} |'
                for name, result in config.results.items())
    temporary = config.artifacts / 'results.md.tmp'
    temporary.write_text('\n'.join(rows) + '\n')
    temporary.replace(config.artifacts / 'results.md')


def pytest_sessionfinish(session, exitstatus):
    config = session.config
    write_results(config)
    if not config.option.collectonly and any(r['status'] in ('blocked', 'planned') for r in config.results.values()):
        session.exitstatus = pytest.ExitCode.TESTS_FAILED


def pytest_terminal_summary(terminalreporter):
    terminalreporter.write_sep('=', f'VM artifacts: {terminalreporter.config.artifacts}')
    counts = {}
    for result in terminalreporter.config.results.values():
        counts[result['status']] = counts.get(result['status'], 0) + 1
    terminalreporter.write_line(', '.join(f'{name.upper()}: {count}' for name, count in counts.items()))
