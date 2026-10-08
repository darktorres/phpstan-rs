<?php

function bump(): void
{
    Counter::$n = 2;
}

bump();
