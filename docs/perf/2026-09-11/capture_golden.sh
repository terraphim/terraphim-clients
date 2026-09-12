#!/usr/bin/env bash
# Capture normalised golden outputs for the #253 workloads and write checksums.
# Normalisation: drop the self-reported latency line (timing, not behaviour).
# agent-search scores are NOT normalised: their run-to-run variance is a
# finding under investigation; the golden is the majority output of 5 runs.
set -u; G=/private/tmp/claude-501/ubx-target-perf/golden; source $G/workloads.sh; unset OPENROUTER_API_KEY
mkdir -p $G/outputs; : > $G/golden_checksums.txt
for n in GREP_CODE GREP_KG AGENT_SEARCH MEM_RETRIEVE; do c="W_$n"
  for r in 1 2 3 4 5; do eval "${!c}" 2>/dev/null | sed -E '/^Search latency:/d' > $G/outputs/$n.r$r; done
  best=$(shasum -a 256 $G/outputs/$n.r? | awk '{print $1}' | sort | uniq -c | sort -rn | head -1)
  cnt=${best%% *}; cnt=$(echo $cnt); sha=${best##* }
  for r in 1 2 3 4 5; do [ "$(shasum -a 256 < $G/outputs/$n.r$r)" = "$sha  -" ] && { cp $G/outputs/$n.r$r $G/outputs/$n.golden; break; }; done
  printf "%-13s majority=%s/5\n" $n "$cnt"
  (cd $G/outputs && shasum -a 256 $n.golden >> $G/golden_checksums.txt)
done
(cd $G/outputs && shasum -a 256 -c $G/golden_checksums.txt)
