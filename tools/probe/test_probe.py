import unittest
import sys
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
import probe


class CpufreqProbeTests(unittest.TestCase):
    def test_collect_sysfs_includes_read_only_amd_pstate_epp_evidence(self):
        values = {
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver": "amd-pstate-epp",
            "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences": (
                "default performance balance_performance balance_power power custom"
            ),
            "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference": "power",
            "/sys/devices/system/cpu/cpu0/cpufreq/boost": "0",
        }

        with patch.object(probe, "read_file", side_effect=values.get), patch.object(
            probe, "file_stat", return_value=None
        ), patch.object(probe.platform, "release", return_value="test"):
            result = probe.collect_sysfs()

        self.assertEqual(result["cpufreq"]["driver"], "amd-pstate-epp")
        self.assertEqual(result["cpufreq"]["energy_performance_preference"], "power")
        self.assertEqual(result["cpufreq"]["boost"], "0")

    def test_collect_sysfs_keeps_missing_cpufreq_evidence_unavailable(self):
        with patch.object(probe, "read_file", return_value=None), patch.object(
            probe, "file_stat", return_value=None
        ), patch.object(probe.platform, "release", return_value="test"):
            result = probe.collect_sysfs()

        self.assertEqual(
            result["cpufreq"],
            {
                "driver": None,
                "energy_performance_available_preferences": None,
                "energy_performance_preference": None,
                "boost": None,
            },
        )


if __name__ == "__main__":
    unittest.main()
