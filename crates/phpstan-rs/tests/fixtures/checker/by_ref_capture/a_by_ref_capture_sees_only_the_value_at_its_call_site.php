<?php
$data = null;
$fn = function () use (&$data): void {
    strlen($data);
};
$data = new ArrayObject();
$fn();
