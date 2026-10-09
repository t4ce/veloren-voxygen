#!/usr/bin/env python3
"""Check fresh Voxy heartbeat evidence; never invent absent hardware results."""
import argparse
import json
import math
from pathlib import Path
import re

MODES = ('near', 'both', 'far')
REQUIRED = set('run_us stream_run_us sequence mode seconds retired published published_fps ready_chunk_frames near_chunks far_vertices far_ready cpu_upload_bytes frame_to_publish_p95_us frame_to_publish_max_us retries window_warm_failures budget_fallback width height new_near_revisions receipt_samples chunk_receipt_to_first_publish_p95_us boundary'.split())


def parse(log):
    rows = []
    malformed = 0
    for line in log.splitlines():
        if 'terrain-heartbeat:' not in line:
            continue
        row = dict(re.findall(r'(\w+)=([^\s]+)', line.split('terrain-heartbeat:', 1)[1]))
        try:
            if not REQUIRED.issubset(row) or row['mode'] not in MODES:
                raise ValueError('missing fields')
            for key in REQUIRED - {'mode', 'far_ready', 'budget_fallback', 'boundary'}:
                row[key] = float(row[key]) if key in {'seconds', 'published_fps'} else int(row[key])
            if row['far_ready'] not in ('true', 'false') or row['budget_fallback'] not in ('true', 'false'):
                raise ValueError('invalid boolean')
            if any(not math.isfinite(row[key]) or row[key] < 0 for key in REQUIRED if isinstance(row[key], (int, float))):
                raise ValueError('negative counter')
            rows.append(row)
        except (ValueError, OverflowError):
            malformed += 1
    return rows, malformed


def warm_proof(log, stream, minimum_crossings):
    fields = set('run_us near_ready warm_ready warm_target resident pending requests ready_crossings cold_crossings missing_at_crossings teleports timeouts request_to_decoded_samples request_to_decoded_p95_us'.split())
    rows = []
    bad = 0
    for line in log.splitlines():
        if 'terrain-prefetch:' not in line:
            continue
        row = dict(re.findall(r'(\w+)=([^\s]+)', line.split('terrain-prefetch:', 1)[1]))
        try:
            if int(row.get('run_us', -1)) != stream:
                continue
            if not fields.issubset(row) or row.get('boundary') != 'decoded-resident' or row.get('protocol') != 'unchanged':
                raise ValueError('incomplete warm evidence')
            row = {key: int(row[key]) for key in fields}
            if any(value < 0 for value in row.values()):
                raise ValueError('invalid counter')
            rows.append(row)
        except ValueError:
            bad += 1
    if not rows:
        return {'pass': False, 'reason': 'No warm evidence for this terrain stream'}
    latest = rows[-1]
    monotonic = all(all(b[key] >= a[key] for key in ['requests', 'ready_crossings', 'cold_crossings', 'missing_at_crossings', 'teleports', 'timeouts'])
                    for a, b in zip(rows, rows[1:]))
    valid = (bad == 0 and monotonic and latest['ready_crossings'] >= minimum_crossings
             and all(row['cold_crossings'] == 0 and row['missing_at_crossings'] == 0 and row['pending'] <= 12 and row['resident'] <= 45 for row in rows)
             and latest['near_ready'] == 9 and latest['warm_ready'] == latest['warm_target']
             and latest['request_to_decoded_samples'] > 0)
    return {'pass': valid, 'stream_run_us': stream, 'boundary': 'decoded-resident',
            'minimum_warm_crossings': minimum_crossings, 'ready_crossings': latest['ready_crossings'],
            'cold_crossings': latest['cold_crossings'], 'teleports': latest['teleports'],
            'timeouts': latest['timeouts'], 'request_to_decoded_p95_us': latest['request_to_decoded_p95_us'],
            'warm_ready': latest['warm_ready'], 'warm_target': latest['warm_target'], 'malformed_records': bad}


def evaluate(rows, min_seconds=30., min_fps=29., near_chunks=9, frame_budget_us=33333, receipt_budget_us=2000000, warm_evidence=None):
    if not rows:
        return {'pass': False, 'reason': 'No complete heartbeat evidence', 'modes': {}}
    run = rows[-1]['run_us']
    stream = rows[-1]['stream_run_us']
    rows = [r for r in rows if r['run_us'] == run and r['stream_run_us'] == stream]
    if warm_evidence is not None and warm_evidence.get('stream_run_us') != stream:
        warm_evidence = {'pass': False, 'reason': 'Warm evidence belongs to a different terrain stream'}
    summary = {'run_us': run, 'stream_run_us': stream, 'boundary': 'gpu-retired+ui4-published', 'physical_display_proven': False,
               'target_fps': 30, 'min_fps': min_fps, 'frame_budget_us': frame_budget_us,
               'minimum_consecutive_seconds_per_mode': min_seconds, 'expected_near_chunks': near_chunks,
               'modes': {}, 'pass': True}
    extents = set()
    for mode in MODES:
        selected = [r for r in rows if r['mode'] == mode]
        longest = current = 0.
        valid_rows = []
        previous_sequence = None
        for row in rows:
            if previous_sequence is not None and row['sequence'] != previous_sequence + 1:
                current = 0.
            previous_sequence = row['sequence']
            steady = row['new_near_revisions'] == 0
            expected = near_chunks if mode != 'far' else 0
            ready = row['near_chunks'] == expected and (mode == 'near' or row['far_ready'] == 'true')
            valid = (row['mode'] == mode and steady and ready and row['seconds'] >= 1.9
                     and row['published'] > 0 and row['retired'] >= row['published']
                     and row['published_fps'] >= min_fps
                     and row['frame_to_publish_p95_us'] <= frame_budget_us
                     and row['ready_chunk_frames'] == expected * row['published']
                     and row['cpu_upload_bytes'] >= 80 * row['retired']
                     and row['cpu_upload_bytes'] <= 80 * (row['retired'] + row['retries'])
                     and row['window_warm_failures'] == 0 and row['budget_fallback'] == 'false'
                     and row['width'] > 0 and row['height'] > 0
                     and row['boundary'] == summary['boundary'])
            if valid:
                extent = (row['width'], row['height'])
                if valid_rows and (valid_rows[-1]['width'], valid_rows[-1]['height']) != extent:
                    current = 0.
                current += row['seconds']
                longest = max(longest, current)
                valid_rows.append(row)
                extents.add(extent)
            else:
                current = 0.
        receipts = [r for r in selected if r['receipt_samples'] > 0]
        receipt_p95_max = max((r['chunk_receipt_to_first_publish_p95_us'] for r in receipts), default=None)
        enough = longest >= min_seconds
        receipt_ok = mode == 'far' or (warm_evidence is not None and warm_evidence.get('pass') is True) or (receipt_p95_max is not None and receipt_p95_max <= receipt_budget_us)
        result = {'pass': enough and receipt_ok, 'windows': len(selected), 'valid_steady_windows': len(valid_rows),
                  'longest_valid_seconds': round(longest, 3), 'receipt_samples': sum(r['receipt_samples'] for r in receipts),
                  'worst_window_receipt_p95_us': receipt_p95_max,
                  'worst_window_frame_p95_us': max((r['frame_to_publish_p95_us'] for r in selected), default=None),
                  'worst_frame_to_publish_us': max((r['frame_to_publish_max_us'] for r in selected), default=None)}
        summary['modes'][mode] = result
        summary['pass'] &= result['pass']
    if warm_evidence is not None:
        summary['warm_evidence'] = warm_evidence
        summary['pass'] &= warm_evidence.get('pass') is True
    summary['same_extent'] = len(extents) == 1
    summary['pass'] &= summary['same_extent']
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('--seconds', type=float, default=30.)
    parser.add_argument('--min-fps', type=float, default=29.)
    parser.add_argument('--near-chunks', type=int, default=9)
    parser.add_argument('--receipt-budget-us', type=int, default=2000000)
    parser.add_argument('--require-warm-crossings', type=int, default=0, help='Require decoded data to be ready at N normal chunk crossings; this supplies the data-readiness proof for all render modes.')
    args = parser.parse_args()
    log = args.log.read_text(errors='replace')
    rows, malformed = parse(log)
    warm = warm_proof(log, rows[-1]['stream_run_us'] if rows else None, args.require_warm_crossings) if args.require_warm_crossings > 0 else None
    result = evaluate(rows, args.seconds, args.min_fps, args.near_chunks, receipt_budget_us=args.receipt_budget_us, warm_evidence=warm)
    result['malformed_heartbeats'] = malformed
    result['pass'] &= malformed == 0
    print(json.dumps(result, indent=2))
    return 0 if result['pass'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
