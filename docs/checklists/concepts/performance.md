# Performance

Performance is what the change costs to run: latency, throughput, memory, allocation churn. It
matters in proportion to how hot the path is, which is why the review question is "what is the
cost here, and is this path hot?" rather than "is this fast?".

An optimization pass is only worth its risk when there is a measurement behind it: a benchmark,
a profile, or a production signal. Without a number, "optimization" is a rewrite with extra
steps and no way to tell whether it helped.

synonyms:: performance, optimization, optimisation, latency, throughput, benchmark, profile, hot path
