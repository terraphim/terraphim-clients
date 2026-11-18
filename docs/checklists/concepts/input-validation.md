# Input validation

Validation decides which inputs are admissible before they reach logic that assumes a shape.
Sanitization escapes what must pass through to another language - SQL, HTML, a shell - where a
stray character changes the meaning of the program.

The ordering matters: validate at the boundary against an allowlist, and escape at the point of
use. Treating a value as safe because it was "validated" earlier is the defect.

synonyms:: input validation, sanitization, sanitisation, escaping, parameterized query, prepared statement, allowlist, injection
