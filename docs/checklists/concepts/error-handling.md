# Error handling

Error handling decides what the program does when reality disagrees with the happy path. The
review questions are whether the failure is detected at all, whether it is propagated with enough
context to act on, and whether the recovery path leaves the system in a state someone can reason
about.

Silent swallowing is the common defect: a catch block that logs nothing, a default that turns a
failure into a plausible-looking value, a retry that hides a permanent error behind a timeout.

synonyms:: error handling, exception handling, error path, failure mode, try catch, panic, fallback
