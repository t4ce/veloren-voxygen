#!/usr/bin/env python3
"""Synthetic checker regressions; these are not Intel performance samples."""
import importlib.util
from pathlib import Path
import unittest

root = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('metrics', root / 'tools/terrain_metrics.py')
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)


def evidence():
    rows = []
    for mode in metrics.MODES:
        chunks = 0 if mode == 'far' else 9
        for index in range(16):
            rows.append(dict(run_us=123, stream_run_us=456, sequence=len(rows)+1, mode=mode, seconds=2., retired=60, published=60,
                published_fps=30., ready_chunk_frames=60*chunks, near_chunks=chunks, far_vertices=0 if mode == 'near' else 98304,
                far_ready='false' if mode == 'near' else 'true', cpu_upload_bytes=80*60,
                frame_to_publish_p95_us=15000, frame_to_publish_max_us=20000,
                retries=0, window_warm_failures=0, budget_fallback='false', width=1920, height=1080,
                new_near_revisions=1 if index == 0 and chunks else 0,
                receipt_samples=1 if index == 0 and chunks else 0,
                chunk_receipt_to_first_publish_p95_us=600000 if index == 0 and chunks else 0,
                boundary='gpu-retired+ui4-published'))
    return rows


class Checks(unittest.TestCase):
    def test_complete_three_mode_evidence_and_no_fabricated_physical_receipt(self):
        result = metrics.evaluate(evidence())
        self.assertTrue(result['pass'])
        self.assertFalse(result['physical_display_proven'])

    def test_stall_or_missing_window_breaks_consecutive_proof(self):
        for missing in (False, True):
            rows = evidence()
            if missing:
                del rows[9]
            else:
                rows[9].update(published=0, published_fps=0, ready_chunk_frames=0)
            self.assertFalse(metrics.evaluate(rows)['pass'])

    def test_mesh_reuploads_partial_coverage_and_budget_fallback_cannot_pass(self):
        for field, value in [('cpu_upload_bytes', 4_000_000), ('near_chunks', 1), ('budget_fallback', 'true'),
                             ('frame_to_publish_p95_us', 40000), ('published_fps', 28.)]:
            rows = evidence()
            rows[9][field] = value
            self.assertFalse(metrics.evaluate(rows)['pass'], field)

    def test_missing_receipts_resolution_changes_or_old_run_cannot_fill_gaps(self):
        rows = evidence()
        rows[0]['receipt_samples'] = 0
        self.assertFalse(metrics.evaluate(rows)['pass'])
        rows = evidence()
        for r in rows[16:32]:
            r['width'] = 1280
        self.assertFalse(metrics.evaluate(rows)['pass'])
        rows = evidence()
        rows[-1]['run_us'] = 124
        self.assertFalse(metrics.evaluate(rows)['pass'])

    def test_prefetched_crossings_prove_readiness_without_inventing_new_receipt_samples(self):
        rows = evidence()
        for row in rows:
            row['receipt_samples'] = 0
        line = 'terrain-prefetch: run_us=456 near_ready=9 warm_ready=16 warm_target=16 resident=30 pending=0 requests=40 ready_crossings=5 cold_crossings=0 missing_at_crossings=0 teleports=0 timeouts=0 request_to_decoded_samples=32 request_to_decoded_p95_us=500000 boundary=decoded-resident protocol=unchanged'
        proof = metrics.warm_proof(line, 456, 5)
        self.assertTrue(proof['pass'])
        self.assertTrue(metrics.evaluate(rows, warm_evidence=proof)['pass'])
        self.assertFalse(metrics.evaluate(rows)['pass'])
        for changed in [line.replace('cold_crossings=0', 'cold_crossings=1'),
                        line.replace('ready_crossings=5', 'ready_crossings=4'),
                        line.replace('run_us=456', 'run_us=123'),
                        line.replace('warm_ready=16', 'warm_ready=12')]:
            self.assertFalse(metrics.evaluate(rows, warm_evidence=metrics.warm_proof(changed, 456, 5))['pass'])
        rows[-1]['stream_run_us'] = 789
        self.assertFalse(metrics.evaluate(rows, warm_evidence=proof)['pass'])

    def test_parser_accepts_log_prefixes_rejects_truncation_and_nonfinite_values(self):
        row = evidence()[0]
        line = '[important apps] terrain-heartbeat: ' + ' '.join(f'{k}={v}' for k,v in row.items())
        parsed, bad = metrics.parse(line)
        self.assertEqual(bad, 0)
        self.assertEqual(parsed, [row])
        self.assertEqual(metrics.parse(line.split('near_chunks=')[0])[1], 1)
        self.assertEqual(metrics.parse(line.replace('seconds=2.0', 'seconds=nan'))[1], 1)
        self.assertFalse(metrics.evaluate([])['pass'])


unittest.main()
