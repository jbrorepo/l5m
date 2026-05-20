import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

from l5m_client import L5MClient


class L5MClientTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.dir = Path(cls.tmp.name)
        cls.input = cls.dir / "capsules.json"
        cls.segment = cls.dir / "test.segment"
        cls.input.write_text(
            json.dumps(
                [
                    {
                        "capsule_id": "1",
                        "tenant_id": 1,
                        "claim": "Production backups are retained for 35 days.",
                        "evidence": "Approved backup policy.",
                        "source_id": 10,
                        "valid_from": 1,
                        "observed_at": 1,
                        "last_verified_at": 1,
                        "context_mask": "0x1",
                        "policy_mask": "0xffff",
                        "trust_level": 8,
                        "classification": 1,
                        "poison_risk": 0,
                    }
                ]
            ),
            encoding="utf-8",
        )
        subprocess.run(
            [
                "cargo",
                "run",
                "-q",
                "-p",
                "l5m-cli",
                "--",
                "compile",
                "--input",
                str(cls.input),
                "--output",
                str(cls.segment),
                "--epoch",
                "1",
            ],
            cwd=ROOT,
            check=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_client_queries_and_closes(self):
        client = L5MClient(segment=self.segment, cwd=ROOT)
        response = client.query(
            {
                "query": "How long are production backups retained?",
                "tenant_id": 1,
                "as_of": 10,
                "context_mask": "0x1",
                "policy_mask": "0xffff",
                "trust_floor": 4,
                "max_capsules": 8,
                "max_tokens": 1024,
                "mode": "L5m",
            }
        )
        client.close()

        self.assertEqual(
            response["frame"]["capsules"][0]["claim"],
            "Production backups are retained for 35 days.",
        )
        self.assertIsNone(client.process)

    def test_client_context_manager_closes_process(self):
        with L5MClient(segment=self.segment, cwd=ROOT) as client:
            response = client.query(
                {
                    "query": "production backups",
                    "tenant_id": 1,
                    "as_of": 10,
                    "context_mask": "0x1",
                    "policy_mask": "0xffff",
                    "trust_floor": 4,
                    "max_capsules": 8,
                    "max_tokens": 1024,
                    "mode": "L5m",
                }
            )
            self.assertIn("frame", response)
        self.assertIsNone(client.process)

    def test_missing_segment_raises_clear_error(self):
        missing = self.dir / "missing.segment"
        with self.assertRaisesRegex(FileNotFoundError, "segment does not exist"):
            L5MClient(segment=missing, cwd=ROOT).query(
                {
                    "query": "production backups",
                    "tenant_id": 1,
                    "as_of": 10,
                    "context_mask": "0x1",
                    "policy_mask": "0xffff",
                    "trust_floor": 4,
                    "max_capsules": 8,
                    "max_tokens": 1024,
                    "mode": "L5m",
                }
            )


if __name__ == "__main__":
    unittest.main()
