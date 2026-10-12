<?php
// Merges the connected Redis facts of redis-connected.php into a copy of the builtin-runs table.
// Each (method, argument) gets one method row per hook any serializer ran there: the union over
// serializers, since the table has no configuration dimension. The probed rows stay as they are.
// Run: php merge-redis.php FACTS.tsv TABLE.tsv OUT.tsv

[, $facts, $table, $out] = $argv;

$hooks = [];
foreach (file($facts, FILE_IGNORE_NEW_LINES) as $line) {
    [$method, $at, , $seen] = explode("\t", $line, 5);
    if ($seen === '-') {
        continue;
    }
    foreach (explode(',', $seen) as $hook) {
        $hooks["redis::$method\t$at\tmethod\t$hook"] = true;
    }
}

$rows = file($table, FILE_IGNORE_NEW_LINES);
$present = array_flip($rows);
$added = 0;
foreach (array_keys($hooks) as $row) {
    if (!isset($present[$row])) {
        $rows[] = $row;
        $added++;
    }
}
sort($rows, SORT_STRING);
file_put_contents($out, implode("\n", $rows) . "\n");
echo "added $added method rows from ", count($hooks), " facts\n";
