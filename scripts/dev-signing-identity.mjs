// A stable, local code-signing identity for development builds.
//
// WHY THIS EXISTS
//
// macOS privacy grants (Bluetooth, Accessibility…) are keyed on the app's
// designated code requirement. An ad-hoc signed app has no certificate, so its
// requirement is its cdhash — and every rebuild is a new cdhash, a new app,
// and a fresh Bluetooth prompt. Observed 2026-09-12: reinstalling a build
// silently stopped the presence pipeline with "STATE unauthorized".
//
// Signed with ANY certificate, the requirement becomes identifier + certificate,
// which survives rebuilds. A Developer ID is not needed for this; TCC never
// consults Gatekeeper or notarization. A self-signed certificate created once
// in the login keychain is enough.
//
// Idempotent: finds "Outsie Dev" if it exists, creates it otherwise. Prints
// the identity name on stdout; prints nothing and exits 0 if it could not be
// made, so callers can fall back to an unsigned build.
import { spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir, homedir } from 'node:os'
import path from 'node:path'

export const IDENTITY = 'Outsie Dev'
const KEYCHAIN = path.join(homedir(), 'Library/Keychains/login.keychain-db')

function run(cmd, args, input) {
  return spawnSync(cmd, args, { encoding: 'utf8', input })
}

export function findIdentity() {
  const r = run('security', ['find-identity', '-v', '-p', 'codesigning'])
  return (r.stdout || '').includes(`"${IDENTITY}"`) ? IDENTITY : null
}

export function ensureIdentity() {
  const found = findIdentity()
  if (found) return found
  const dir = mkdtempSync(path.join(tmpdir(), 'outsie-sign-'))
  try {
    const cnf = path.join(dir, 'openssl.cnf')
    writeFileSync(cnf, [
      '[req]', 'distinguished_name = dn', 'x509_extensions = codesign', 'prompt = no',
      '[dn]', `CN = ${IDENTITY}`,
      '[codesign]', 'keyUsage = critical, digitalSignature',
      'extendedKeyUsage = critical, codeSigning',
      'basicConstraints = critical, CA:false', 'subjectKeyIdentifier = hash', '',
    ].join('\n'))
    const key = path.join(dir, 'key.pem'), cert = path.join(dir, 'cert.pem'), p12 = path.join(dir, 'id.p12')
    let r = run('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '3650',
      '-keyout', key, '-out', cert, '-config', cnf, '-extensions', 'codesign'])
    if (r.status !== 0) { console.error(r.stderr); return null }
    // Export → import: the only way `security` accepts a private key. OpenSSL 3
    // writes a PBES2/AES container by default, which exports fine and then
    // fails macOS's import with "MAC verification failed" — so the fallback
    // keys on the IMPORT failing, and re-exports with -legacy (RC2/3DES), which
    // LibreSSL produces by default and does not have a flag for.
    // The passphrase guards a p12 that is written and deleted inside this one
    // run, so it is fresh every time rather than a literal in public source.
    const pass = randomBytes(16).toString('hex')
    const p12Args = ['pkcs12', '-export', '-inkey', key, '-in', cert, '-out', p12, '-passout', `pass:${pass}`, '-name', IDENTITY]
    const importArgs = ['import', p12, '-k', KEYCHAIN, '-P', pass, '-T', '/usr/bin/codesign', '-T', '/usr/bin/security']
    r = run('openssl', p12Args)
    if (r.status !== 0) { console.error(r.stderr); return null }
    r = run('security', importArgs)
    if (r.status !== 0) {
      const legacy = run('openssl', [...p12Args, '-legacy'])
      if (legacy.status !== 0) { console.error(r.stderr, legacy.stderr); return null }
      r = run('security', importArgs)
      if (r.status !== 0) { console.error(r.stderr); return null }
    }
    // User-domain trust for code signing, so codesign treats it as a valid
    // identity. No admin rights: nothing outside this login keychain changes.
    r = run('security', ['add-trusted-cert', '-r', 'trustRoot', '-p', 'codeSign', '-k', KEYCHAIN, cert])
    if (r.status !== 0) { console.error(r.stderr); return null }
    return findIdentity()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  const id = ensureIdentity()
  if (id) process.stdout.write(id + '\n')
  else process.stderr.write('no signing identity; builds will be ad-hoc (Bluetooth grant will not survive rebuilds)\n')
}
