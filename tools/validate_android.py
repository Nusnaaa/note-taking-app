#!/usr/bin/env python3
"""Run actual ARM64, JVM/JNI and Android-module checks; missing tools fail, never skip.

Provision JDK 17, Gradle 8.9, Android SDK 35/build tools 35 and NDK 27 first.
No emulator/device evidence is implied by desktop JNI or Robolectric checks.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import urllib.parse
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--sdk', default=os.environ.get('ANDROID_HOME'))
    p.add_argument('--jdk', default=os.environ.get('JAVA_HOME'))
    p.add_argument('--gradle', default=shutil.which('gradle'))
    p.add_argument('--online', action='store_true', help='Allow build-tool dependency provisioning; application content is never supplied.')
    args=p.parse_args()
    if not all([args.sdk,args.jdk,args.gradle]): p.error('SDK, JDK and Gradle paths are required')
    dest=ROOT/'docs/evidence/latest';dest.mkdir(parents=True,exist_ok=True)
    cargo=shutil.which('cargo') or str(Path.home()/'.cargo/bin/cargo')
    sdk=Path(args.sdk).resolve();jdk=Path(args.jdk).resolve()
    compiler=sdk/'ndk/27.0.12077973/toolchains/llvm/prebuilt/linux-x86_64/bin'
    env=dict(os.environ,ANDROID_HOME=str(sdk),JAVA_HOME=str(jdk),CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='1',CARGO_PROFILE_RELEASE_CODEGEN_UNITS='1')
    env['CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER']=str(compiler/'aarch64-linux-android30-clang')
    env['CC_aarch64_linux_android']=env['CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER']
    env['AR_aarch64_linux_android']=str(compiler/'llvm-ar')
    git=lambda *a:subprocess.check_output(['git',*a],cwd=ROOT,text=True).strip()
    paths=git('ls-files').splitlines()
    report={'source_commit':git('rev-parse','HEAD'),'working_tree_before':git('status','--porcelain'),'source_sha256':{v:hashlib.sha256((ROOT/v).read_bytes()).hexdigest() for v in paths if not v.startswith('docs/evidence/')},'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':[],'device_tests':'NOT RUN; Robolectric simulates API 35; JNI runs on desktop JVM','environment':{k:env[k] for k in ['ANDROID_HOME','JAVA_HOME','CARGO_INCREMENTAL','CARGO_BUILD_JOBS','CARGO_PROFILE_RELEASE_CODEGEN_UNITS','CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER','CC_aarch64_linux_android','AR_aarch64_linux_android']}}
    report['working_tree_before']=subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).rstrip()
    report['full_working_tree_before']=report['working_tree_before']
    report['working_tree_before']='\n'.join(line for line in report['working_tree_before'].splitlines() if not line[3:].startswith('docs/evidence/'))
    def save(): (dest/'android-validation.json').write_text(json.dumps(report,indent=2)+'\n')
    def run(name,command):
        result=subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        (dest/f'{name}.log').write_text('$ '+shlex.join(command)+'\nexit_code='+str(result.returncode)+'\n\n'+result.stdout)
        report['checks'].append({'name':name,'command':command,'exit_code':result.returncode,'result':'PASS' if result.returncode==0 else 'FAIL'})
        save();print(name+': '+report['checks'][-1]['result'],flush=True)
        if result.returncode: print(result.stdout[-4000:]);raise SystemExit(result.returncode)
    run('android-native',[cargo,'build','-p','stem-bridge','--target','aarch64-linux-android','--release','--locked','--offline'])
    native=ROOT/'target/aarch64-linux-android/release/libstem_bridge.so'
    run('android-elf',['readelf','-h',str(native)])
    target=ROOT/'target/android-jni/arm64-v8a';target.mkdir(parents=True,exist_ok=True);shutil.copy2(native,target/native.name)
    run('jni-library',[cargo,'build','-p','stem-bridge','--locked','--offline'])
    target=ROOT/'target/jvm-test';target.mkdir(exist_ok=True)
    run('jni-compile',[str(jdk/'bin/javac'),'-d',str(target),'android/backend/src/main/java/org/stemnotebook/backend/NativeBridge.java','bridge/tests/java/BoundarySmoke.java'])
    run('jni-smoke',[str(jdk/'bin/java'),'-Xcheck:jni','-Djava.library.path=target/debug','-cp',str(target),'BoundarySmoke'])
    gradle=[args.gradle,'--no-daemon','-p','android',':backend:assembleDebug',':backend:testDebugUnitTest',':backend:lintDebug','--console=plain','--rerun-tasks']
    if not args.online: gradle.append('--offline')
    proxy=urllib.parse.urlsplit(os.environ.get('HTTPS_PROXY',''))
    if proxy.hostname and proxy.port:
        gradle += [f'-Dhttps.proxyHost={proxy.hostname}',f'-Dhttps.proxyPort={proxy.port}',f'-Dhttp.proxyHost={proxy.hostname}',f'-Dhttp.proxyPort={proxy.port}']
    if Path('/etc/ssl/certs/java/cacerts').is_file(): gradle.append('-Djavax.net.ssl.trustStore=/etc/ssl/certs/java/cacerts')
    run('android-gradle',gradle)
    suites=[ET.parse(path).getroot() for path in (ROOT/'android/backend/build/test-results/testDebugUnitTest').glob('TEST-*.xml')]
    totals={key:sum(int(s.get(key,'0')) for s in suites) for key in ['tests','failures','errors','skipped']}
    if totals['tests']<10 or any(totals[k] for k in ['failures','errors','skipped']): raise RuntimeError('Missing, failed or skipped Android unit checks')
    manifest=ET.parse(ROOT/'android/backend/build/intermediates/merged_manifest/debug/processDebugManifest/AndroidManifest.xml').getroot()
    ns='{http://schemas.android.com/apk/res/android}';app=manifest.find('application')
    assert app is not None and app.get(ns+'allowBackup')=='false'
    # AAR manifests deliberately retain tools merge markers. A removal directive
    # is not a grant; this checks the library contract, not a future APK manifest.
    permissions=manifest.findall('uses-permission')
    internet=[node for node in permissions if node.get(ns+'name')=='android.permission.INTERNET']
    assert internet and all(node.get('{http://schemas.android.com/tools}node')=='remove' for node in internet)
    assert app.get(ns+'usesCleartextTraffic')=='false'
    services=app.findall('service');pdf_services=[s for s in services if s.get(ns+'name','').endswith('.PdfWorkerService')]
    assert len(pdf_services)==1 and pdf_services[0].get(ns+'isolatedProcess')=='true' and pdf_services[0].get(ns+'exported')=='false'
    aar=ROOT/'android/backend/build/outputs/aar/backend-debug.aar'
    with zipfile.ZipFile(aar) as bundle:
        packaged=bundle.read('jni/arm64-v8a/libstem_bridge.so')
        assert packaged[:6]==b'\x7fELF\x02\x01' and int.from_bytes(packaged[18:20],'little')==183
    report['aar_sha256']=hashlib.sha256(aar.read_bytes()).hexdigest()
    report['packaged_arm64_library_bytes']=len(packaged)
    report['android_tests']=totals;report['jni_assertions']=7;report['merged_manifest_policy']='PASS';report['native_sha256']=hashlib.sha256(native.read_bytes()).hexdigest()
    report['manifest_scope']='AAR merge contract only: INTERNET removal marker, no backup/transfer, no cleartext, isolated non-exported PDF service. Final APK/device enforcement NOT RUN.'
    report['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();save();print(json.dumps(totals))
    return 0

if __name__=='__main__': raise SystemExit(main())
