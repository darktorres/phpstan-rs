<?php
interface HasRun
{
    public function run(): void;
}

class Base
{
    private function run(): void
    {
    }

    public function go(): void
    {
        $this->run();
    }
}

function f(Base&HasRun $x): void
{
    $x->run();
}

function g(HasRun&Base $x): void
{
    $x->run();
}
