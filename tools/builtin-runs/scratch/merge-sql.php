<?php
// Merges the facts of sql-connected.php and scope-facts.php into a table: each builtin and argument gets one
// method row per hook it ran, or a calls row when its callable was called. Facts have one configuration, so there is no union.
// Run: php merge-sql.php FACTS.tsv TABLE.tsv OUT.tsv

[, $facts, $table, $out] = $argv;

$hooks = [];
foreach (file($facts, FILE_IGNORE_NEW_LINES) as $line) {
    [$builtin, $at, $seen] = explode("\t", $line, 4);
    if ($seen === '-') {
        continue;
    }
    foreach (explode(',', $seen) as $hook) {
        $hooks[$hook === 'calls' ? "$builtin\t$at\tcalls\t" : "$builtin\t$at\tmethod\t$hook"] = true;
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
