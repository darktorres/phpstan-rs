<?php

function usesGlobal()
{
    global $config;
    return $config;
}

function outputParam(string $text)
{
    if (preg_match('/(\d+)/', $text, $matches)) {
        return $matches[1];
    }
    return null;
}

function chainedAssign()
{
    $a = $b = 5;
    return $a + $b;
}

function keyedDestructure(array $data)
{
    ['name' => $n] = $data;
    return $n;
}

function fullyDefinedBothBranches(bool $cond)
{
    if ($cond) {
        $x = 5;
    } else {
        $x = 10;
    }
    return $x;
}

function withExtract(array $data)
{
    extract($data);
    return $whateverKeyDataHappensToHave;
}

echo usesGlobal();
echo outputParam("abc123");
echo chainedAssign();
echo keyedDestructure(["name" => "Ada"]);
echo fullyDefinedBothBranches(true);
echo withExtract(["whateverKeyDataHappensToHave" => "hi"]);
