# Security

Security is the property that the system resists, or survives, deliberate misuse: an attacker
reaching data they should not see, executing a path they were not meant to reach, or exhausting
a resource. A security review asks who can reach the change under review and what they can do
once there.

For a code review the useful questions are concrete: does this change widen who can call it,
does it trust input from across a boundary, does it log or leak anything sensitive, and does it
add a dependency whose compromise would matter. A "security check" that lists none of those is a
word, not a review.

synonyms:: security, security check, security review, threat model, attack surface, vulnerability, cve, hardening, exploit
