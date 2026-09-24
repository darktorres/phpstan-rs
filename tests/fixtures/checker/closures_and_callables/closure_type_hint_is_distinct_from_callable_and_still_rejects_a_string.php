<?php
            function needsClosure(Closure $fn): void {}
            function test(): void { needsClosure('strlen'); }
        