<?php
            function inner(): void {}
            function outer(): void {
                return inner();
            }
        