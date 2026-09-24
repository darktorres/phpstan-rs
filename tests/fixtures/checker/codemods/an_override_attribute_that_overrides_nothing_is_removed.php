<?php
class A {
    #[\Override]
    public function nothing(): void {}
    #[\Override, Deprecated]
    public function other(): void {}
}
