#!/usr/bin/env python3
"""Desktop JNI checks, independently of ARM64/Gradle; never device acceptance."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    parser.add_argument('--r8', type=Path, required=True, help='Local Android build-tools d8.jar')
    args = parser.parse_args()
    destination = (ROOT / args.output).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    cargo = shutil.which('cargo')
    java = shutil.which('java')
    javac = shutil.which('javac')
    if not all((cargo, java, javac)) or not args.r8.is_file():
        parser.error('Rust, JDK and a local R8 JAR are required; nothing is skipped')
    git = lambda *args: subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()
    paths = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT).decode().split('\0')
    report = {
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'source_commit': git('rev-parse', 'HEAD'),
        'working_tree_before': git('status', '--porcelain'),
        'source_sha256': {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
                          for p in sorted(set(paths)) if p and not p.startswith('docs/evidence/')},
        'scope': 'Actual desktop JVM/JNI and R8 classfile optimization; ARM64, Android, provider and device acceptance NOT RUN',
        'r8_sha256': hashlib.sha256(args.r8.read_bytes()).hexdigest(),
        'checks': [],
    }

    def run(name, command):
        result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (destination / (name + '.log')).write_text('$ ' + shlex.join(command) + '\nexit_code=' + str(result.returncode) + '\n\n' + result.stdout)
        report['checks'].append({'name': name, 'command': command, 'exit_code': result.returncode})
        (destination / 'jni-validation.json').write_text(json.dumps(report, indent=2) + '\n')
        print(name + ': ' + ('PASS' if result.returncode == 0 else 'FAIL'), flush=True)
        if result.returncode:
            print(result.stdout[-6000:], flush=True)
            raise SystemExit(result.returncode)
        return result.stdout

    run('jni-format', [cargo, 'fmt', '--all', '--', '--check'])
    run('jni-lint', [cargo, 'clippy', '-p', 'stem-bridge', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'])
    run('jni-tests', [cargo, 'test', '-p', 'stem-bridge', '--locked', '--offline'])
    run('jni-library', [cargo, 'build', '-p', 'stem-bridge', '--locked', '--offline'])
    with tempfile.TemporaryDirectory(prefix='stem-jni-') as scratch:
        classes = Path(scratch) / 'classes'
        classes.mkdir()
        sources = ['android/backend/src/main/java/org/stemnotebook/backend/NativeBridge.java']
        sources += [str(path.relative_to(ROOT)) for path in sorted((ROOT / 'bridge/tests/java').glob('*.java'))]
        run('jni-compile', [javac, '-d', str(classes), *sources])
        command = [java, '-Xcheck:jni', '-Djava.library.path=target/debug', '-cp']
        smoke = run('jni-smoke', command + [str(classes), 'BoundarySmoke'])
        groups = sum(int(n) for n in re.findall(r'^PASS:.*\((\d+)\)$', smoke, re.MULTILINE))
        if groups < 25:
            raise RuntimeError('Missing JNI fixture groups')
        inputs = Path(scratch) / 'input.jar'
        with zipfile.ZipFile(inputs, 'w') as bundle:
            for path in sorted(classes.rglob('*.class')):
                bundle.write(path, path.relative_to(classes))
        optimized = Path(scratch) / 'shrunk.jar'
        mapping = destination / 'jni-shrinker.map'
        r8 = [java, '-cp', str(args.r8.resolve()), 'com.android.tools.r8.R8']
        run('jni-shrinker-version', r8 + ['--version'])
        jdk = Path(javac).resolve().parent.parent
        run('jni-shrink', r8 + ['--classfile', '--release', '--lib', str(jdk), '--pg-conf', 'android/backend/consumer-rules.pro', '--pg-conf', 'bridge/tests/java/shrinker-entry.pro', '--pg-map-output', str(mapping), '--output', str(optimized), str(inputs)])
        for fixture in ('BackupBoundarySmoke', 'RestoreBoundarySmoke'):
            if not re.search('^' + fixture + ' -> (?!' + fixture + ':)[^\n]+:', mapping.read_text(), re.MULTILINE):
                raise RuntimeError('Fixture was not minified: ' + fixture)
        shrunk = run('jni-shrunk-smoke', command + [str(optimized), 'BoundarySmoke'])
        optimized_groups = sum(int(n) for n in re.findall(r'^PASS:.*\((\d+)\)$', shrunk, re.MULTILINE))
        if optimized_groups != groups:
            raise RuntimeError('Shrunk JNI fixture groups differ')
        report['fixture_groups'] = groups
        report['optimized_sha256'] = hashlib.sha256(optimized.read_bytes()).hexdigest()
    run('jni-traceability', [shutil.which('python3'), 'tools/traceability.py', '--check'])
    report['finished_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (destination / 'jni-validation.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
