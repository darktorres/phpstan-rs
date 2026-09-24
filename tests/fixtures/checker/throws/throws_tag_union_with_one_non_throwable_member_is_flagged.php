<?php
            class NotAThrowable {}
            /** @throws NotAThrowable|\RuntimeException */
            function f(): void {}
        