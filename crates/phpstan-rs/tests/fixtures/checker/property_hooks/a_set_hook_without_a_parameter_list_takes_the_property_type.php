<?php
class S
{
    public string $a {
        set => strtolower($value);
    }

    public string $b {
        set {
            $this->b = strtolower($value);
        }
    }
}

function g(S $s): void
{
    $s->a = 'Q';
    $s->b = 'Q';
}
