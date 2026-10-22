<?php

namespace PhpstanRs\Testing;

/**
 * Where `$value` came from: its data-flow nodes, sorted and comma-separated
 * (`env:$_GET`), as phpstan-rs infers them; the checker reports a mismatch.
 *
 * @param mixed $value
 * @return mixed
 */
function assertFlow(string $expected, $value)
{
    return $value;
}
