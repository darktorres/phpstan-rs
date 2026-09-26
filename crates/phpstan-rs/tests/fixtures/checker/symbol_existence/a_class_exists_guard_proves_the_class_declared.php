<?php
function f(object $o): void
{
    if (class_exists('OptionalDependency')) {
        new OptionalDependency();
        OptionalDependency::create();
        echo OptionalDependency::VERSION;
        if ($o instanceof OptionalDependency) {
        }
        $make = function () {
            return new OptionalDependency();
        };
    }
    if (interface_exists('\OptionalContract')) {
        if ($o instanceof OptionalContract) {
        }
    }
    new OptionalDependency();
}
