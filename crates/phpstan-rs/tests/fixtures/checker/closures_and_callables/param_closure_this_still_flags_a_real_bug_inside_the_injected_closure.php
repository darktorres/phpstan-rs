<?php
            class Empty5841 {}
            /**
             * @param-closure-this Empty5841 $cb
             */
            function withEmpty5841(Closure $cb) { return $cb(); }
            function test(): void {
                withEmpty5841(function () { return $this->totallyUndefinedVar; });
            }
        