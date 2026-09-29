<?php
interface HasLabel
{
    public function label(int $x): string;
}

enum Suit implements HasLabel
{
    case Hearts;

    public function label(string $x): string
    {
        return $x;
    }
}
