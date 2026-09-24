<?php
            class Empty4841 {}
            function test(): void {
                Closure::bind(function () { return $this->totallyUndefinedVar; }, new Empty4841());
            }
        