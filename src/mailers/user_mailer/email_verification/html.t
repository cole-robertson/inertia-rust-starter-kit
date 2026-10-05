<!DOCTYPE html>
<html>
  <head>
    <meta http-equiv="Content-Type" content="text/html; charset=utf-8">
    <style>
      /* Email styles need to be inline */
    </style>
  </head>

  <body>
    <p>Hey there,</p>

<p>This is to confirm that {{ email | escape }} is the email you want to use on your account. If you ever lose your password, that's where we'll email a reset link.</p>

<p><strong>You must hit the link below to confirm that you received this email.</strong></p>

<p><a href="{{ url | escape }}">Yes, use this email for my account</a></p>

<hr>

<p>Have questions or need help? Just reply to this email and our support team will help you sort it out.</p>

  </body>
</html>
