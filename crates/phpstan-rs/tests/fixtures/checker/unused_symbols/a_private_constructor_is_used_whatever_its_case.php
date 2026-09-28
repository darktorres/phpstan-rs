<?php

final class Singleton
{
    private function __CONSTRUCT(private int $count)
    {
    }

    public static function make(): self
    {
        return new self(1);
    }

    public function count(): int
    {
        return $this->count;
    }
}
