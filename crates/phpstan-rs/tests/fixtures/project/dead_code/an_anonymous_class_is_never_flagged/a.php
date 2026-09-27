<?php
interface Greeter { public function greet(): string; }
function make(): Greeter {
    return new class implements Greeter {
        public function greet(): string { return "hi"; }
    };
}
echo make()->greet();
