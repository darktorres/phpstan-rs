<?php
    function f(mixed $m): void {
        $m->run();
        echo $m->name;
        echo $m['key'];
    }
