# Authentication

Authentication establishes *who* is acting: a password, a token, a certificate, an OIDC
assertion, a hardware key. The review questions are about the edges - how the credential is
obtained, how long it lives, how it is stored, and what happens when it turns out to be wrong.

Common defects are quiet ones: a login path that reveals whether an account exists, a token that
outlives the session, a credential cached where another process can read it.

synonyms:: authentication, auth, login, sign in, log in, credential, password, mfa, session token, oidc
