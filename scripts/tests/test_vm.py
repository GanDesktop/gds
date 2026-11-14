"""Exercise scripts/vm.py argument validation and virsh workflow helpers."""

import contextlib
import io
import os
from pathlib import Path
import subprocess
import unittest
from unittest import mock

SCRIPTS_DIR = Path(__file__).resolve().parents[1]

import sys

sys.path.insert(0, str(SCRIPTS_DIR))
import vm  # noqa: E402


class VmScriptTests(unittest.TestCase):
    def test_parse_defaults_and_rejects_invalid_all_or_extra_args(self) -> None:
        _, all_default = vm.parse_args(['all'])
        self.assertEqual(all_default.machine, 'all')
        self.assertEqual(all_default.action, 'status')

        _, machine_default = vm.parse_args(['noctalia'])
        self.assertEqual(machine_default.machine, 'noctalia')
        self.assertEqual(machine_default.action, 'open')
        self.assertEqual(vm.MACHINES['noctalia'], 'arch-noctalia')

        for forbidden in (['all', 'open'], ['all', 'ssh', 'id'], ['all', 'copy', 'a.deb']):
            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as ctx:
                    vm.parse_args(forbidden)
                self.assertEqual(ctx.exception.code, 2)

        for bad in (['ubuntu', 'copy'], ['ubuntu', 'reset', 'extra'], ['all', 'status', 'extra']):
            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as ctx:
                    vm.parse_args(bad)
                self.assertEqual(ctx.exception.code, 2)

    def test_ip_address_falls_back_from_lease_to_agent_and_skips_loopback(self) -> None:
        responses = [
            subprocess.CompletedProcess(['virsh'], 0, stdout=' Name  MAC\n---\n', stderr=''),
            subprocess.CompletedProcess(
                ['virsh'],
                0,
                stdout=(
                    ' lo         00:00:00:00:00:00    ipv4         127.0.0.1/8\n'
                    ' enp1s0     52:54:00:8c:53:ec    ipv4         192.168.122.114/24\n'
                ),
                stderr='',
            ),
        ]
        with mock.patch.object(vm.subprocess, 'run', side_effect=responses) as run_mock:
            address = vm.ip_address('fedora-dev')
        self.assertEqual(address, '192.168.122.114')
        self.assertEqual(
            run_mock.call_args_list[0].args[0],
            [*vm.VIRSH, 'domifaddr', 'fedora-dev', '--source', 'lease'],
        )
        self.assertEqual(
            run_mock.call_args_list[1].args[0],
            [*vm.VIRSH, 'domifaddr', 'fedora-dev', '--source', 'agent'],
        )

    def test_reset_machine_verifies_snapshot_and_reverts(self) -> None:
        calls: list[list[str]] = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd)
            if 'snapshot-info' in cmd:
                return subprocess.CompletedProcess(cmd, 0, stdout='Name: clean-base\n', stderr='')
            return subprocess.CompletedProcess(cmd, 0)

        with mock.patch.object(vm.subprocess, 'run', side_effect=fake_run):
            vm.reset_machine('ubuntu-dev')

        self.assertEqual(
            calls,
            [
                [*vm.VIRSH, 'snapshot-info', 'ubuntu-dev', 'clean-base'],
                [*vm.VIRSH, 'snapshot-revert', 'ubuntu-dev', 'clean-base'],
            ],
        )

        missing = subprocess.CompletedProcess(['virsh'], 1, stdout='', stderr='snapshot not found')
        with mock.patch.object(vm.subprocess, 'run', return_value=missing):
            with self.assertRaises(SystemExit) as ctx:
                vm.reset_machine('ubuntu-dev')
        self.assertIn("Snapshot 'clean-base' not found for 'ubuntu-dev'", str(ctx.exception))

    def test_stop_machine_falls_back_to_acpi_and_waits_for_shut_off(self) -> None:
        states = iter(['running', 'in shutdown', 'shut off'])
        calls: list[list[str]] = []

        def fake_run(cmd, **kwargs):
            calls.append(cmd)
            if 'domstate' in cmd:
                return subprocess.CompletedProcess(cmd, 0, stdout=next(states) + '\n', stderr='')
            if '--mode' in cmd:
                return subprocess.CompletedProcess(cmd, 1, stdout='', stderr='agent not connected')
            return subprocess.CompletedProcess(cmd, 0)

        with (
            mock.patch.object(vm.subprocess, 'run', side_effect=fake_run),
            mock.patch.object(vm.time, 'sleep', return_value=None),
            mock.patch.object(vm.time, 'monotonic', side_effect=[0.0, 1.0, 2.0]),
        ):
            vm.stop_machine('fedora-dev', timeout=10.0)

        self.assertEqual(
            calls,
            [
                [*vm.VIRSH, 'domstate', 'fedora-dev'],
                [*vm.VIRSH, 'shutdown', 'fedora-dev', '--mode', 'agent'],
                [*vm.VIRSH, 'shutdown', 'fedora-dev'],
                [*vm.VIRSH, 'domstate', 'fedora-dev'],
                [*vm.VIRSH, 'domstate', 'fedora-dev'],
            ],
        )

    def test_open_machine_strips_nix_ld_library_path_and_sets_wayland(self) -> None:
        seen_env: dict[str, str] = {}

        def fake_run(cmd, **kwargs):
            if 'domstate' in cmd:
                return subprocess.CompletedProcess(cmd, 0, stdout='running\n', stderr='')
            seen_env.update(kwargs['env'])
            return subprocess.CompletedProcess(cmd, 0)

        parser, _ = vm.parse_args(['ubuntu'])
        with (
            mock.patch.dict(
                os.environ,
                {'WAYLAND_DISPLAY': 'wayland-1', 'IN_NIX_SHELL': 'impure', 'LD_LIBRARY_PATH': '/bad'},
                clear=False,
            ),
            mock.patch.object(vm.shutil, 'which', return_value='/usr/bin/virt-viewer'),
            mock.patch.object(vm.subprocess, 'run', side_effect=fake_run),
        ):
            vm.open_machine('ubuntu-dev', parser)

        self.assertEqual(seen_env.get('GDK_BACKEND'), 'wayland')
        self.assertNotIn('LD_LIBRARY_PATH', seen_env)


if __name__ == '__main__':
    unittest.main()
