<?php

class User
{
    public string $name = "";
}

/**
 * @param array<int, User> $users
 * @return array{count: int, first: string}
 */
function summarize(array $users)
{
    return ["count" => count($users), "first" => "n/a"];
}

/**
 * @param string[] $tags
 * @return array{name: string, tags: string[]}
 */
function makeRecord(string $name, array $tags)
{
    return ["name" => $name, "tags" => $tags];
}

echo summarize([])["count"];
echo makeRecord("widget", ["a", "b"])["name"];
