<?php

function probe(array $row): void
{
    if (isset($maybe)) {
        echo $maybe;
    }
    if (!empty($row["name"])) {
        echo $row["name"];
    }
    $gone = 1;
    unset($gone);
    echo $gone;
}
